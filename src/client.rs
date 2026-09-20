//! Native Rust CLI/MCP client. All authorization and business rules stay on the server.
use crate::client_config::{self, Config};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    time::Duration,
};

struct Client {
    http: reqwest::Client,
    config: Config,
}
impl Client {
    fn new(config: Config) -> Result<Self> {
        ensure!(
            !config.url.is_empty(),
            "Run: joppa login --server https://YOUR_JOPPA_DOMAIN"
        );
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { http, config })
    }
    async fn request(
        &self,
        path: &str,
        payload: Option<&Value>,
        authorized: bool,
    ) -> Result<Option<Value>> {
        let url = format!("{}{path}", self.config.url);
        let mut request = if let Some(body) = payload {
            self.http.post(url).json(body)
        } else {
            self.http.get(url)
        };
        if authorized {
            request = request.bearer_auth(
                self.config
                    .key
                    .as_deref()
                    .context("Not signed in. Run joppa login")?,
            );
        }
        let response = request.send().await.context("Could not reach Joppa")?;
        let status = response.status();
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            bail!("Sign-in expired or revoked. Run joppa login");
        }
        let body: Value = response
            .json()
            .await
            .context("Joppa returned an invalid response")?;
        ensure!(
            status.is_success(),
            "{}",
            body["error"]
                .as_str()
                .unwrap_or("Joppa rejected the request")
        );
        Ok(Some(body))
    }
    async fn tool(&self, name: &str, args: Value) -> Result<Value> {
        let msg = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}});
        let response = self
            .request("/mcp", Some(&msg), true)
            .await?
            .context("Missing MCP response")?;
        ensure!(response.get("error").is_none(), "MCP request rejected");
        let result = &response["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .context("Invalid MCP result")?;
        ensure!(result["isError"] != true, "{text}");
        Ok(serde_json::from_str(text)?)
    }
}
pub async fn run(args: &[String]) -> Result<()> {
    let action = args.first().map(String::as_str).unwrap_or("mcp");
    let mut config = client_config::load()?;
    if action == "login" {
        if args.len() > 1 {
            ensure!(
                args.len() == 3 && args[1] == "--server",
                "usage: joppa login [--server https://YOUR_JOPPA_DOMAIN]"
            );
            let url = client_config::validate_url(&args[2])?;
            if url != config.url {
                config.key = None;
            }
            config.url = url;
        }
        return login(config).await;
    }
    let c = Client::new(config)?;
    match action {
        "mcp" => mcp(c).await,
        "whoami" => {
            println!("{}", c.request("/api/me", None, true).await?.unwrap());
            Ok(())
        }
        "logout" => {
            c.request("/auth/device/revoke", Some(&json!({})), true)
                .await?;
            client_config::save(&Config {
                url: c.config.url,
                key: None,
            })?;
            println!("Signed out. This CLI key is revoked.");
            Ok(())
        }
        "list" => {
            println!("{}", c.tool("joppa_list", json!({})).await?);
            Ok(())
        }
        "read" => {
            let ws = args.get(1).context("usage: joppa read WORKSPACE")?;
            println!("{}", c.tool("joppa_read", json!({"workspace":ws})).await?);
            Ok(())
        }
        "command" => {
            ensure!(
                args.len() == 3,
                "usage: joppa command WORKSPACE REQUEST_ID < command.json"
            );
            let mut raw = String::new();
            std::io::stdin().read_to_string(&mut raw)?;
            let command: Value = serde_json::from_str(&raw)?;
            println!(
                "{}",
                c.tool(
                    "joppa_command",
                    json!({"workspace":args[1],"request_id":args[2],"command":command})
                )
                .await?
            );
            Ok(())
        }
        _ => bail!(
            "Client commands: login [--server URL] | logout | whoami | mcp | list | read WORKSPACE | command WORKSPACE REQUEST_ID"
        ),
    }
}
async fn login(config: Config) -> Result<()> {
    let c = Client::new(config)?;
    let start = c
        .request("/auth/device/start", Some(&json!({})), false)
        .await?
        .context("Missing connection code")?;
    let code = start["code"].as_str().context("Invalid connection code")?;
    let secret = start["device_secret"]
        .as_str()
        .context("Invalid device secret")?;
    let url = format!("{}/connect?code={code}", c.config.url);
    println!("Open: {url}\nCode: {code}\nApprove this connection in your browser. Waiting…");
    std::io::stdout().flush()?;
    let deadline = std::time::Instant::now()
        + Duration::from_secs(start["expires_in"].as_u64().unwrap_or(600).min(600));
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let response = c
            .request(
                "/auth/device/poll",
                Some(&json!({"code":code,"device_secret":secret})),
                false,
            )
            .await?
            .context("Missing connection result")?;
        if response["status"] == "ready" {
            let key = response["key"].as_str().context("Missing CLI key")?;
            client_config::save(&Config {
                url: c.config.url,
                key: Some(key.into()),
            })?;
            println!("Connected. CLI and MCP are ready.");
            return Ok(());
        }
    }
    bail!("Connection expired. Run joppa login again")
}
async fn mcp(c: Client) -> Result<()> {
    for line in std::io::stdin().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}}),
            ),
            Ok(msg) => match c.request("/mcp", Some(&msg), true).await {
                Ok(value) => value,
                Err(e) if msg.get("id").is_some() => Some(
                    json!({"jsonrpc":"2.0","id":msg["id"],"error":{"code":-32000,"message":e.to_string()}}),
                ),
                Err(e) => {
                    eprintln!("joppa: {e}");
                    None
                }
            },
        };
        if let Some(r) = response {
            println!("{r}");
            std::io::stdout().flush()?;
        }
    }
    Ok(())
}
