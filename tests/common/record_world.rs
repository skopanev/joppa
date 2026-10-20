//! One workspace on its own real server for the record tests. Agent bot, member alex and
//! member another-member belong to it. Outsider is signed in, but is not a member.
//! Criterion ac:a lists the tests of the fixture. The setup writes through the store.
//! The tests write through MCP.
#![allow(dead_code)]

use anyhow::{Context, Result, ensure};
use joppa::{commands::Command as Operation, store};
use serde_json::{Value, json};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// The criterion of each fixture.
pub const A: &str = "ac:a";

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub struct World {
    _server: Server,
    _dir: std::path::PathBuf,
    pub root: String,
    pub ws: String,
    pub owner: String,
    pub bot: String,
    pub alex: String,
    pub outsider: String,
    pub pool: deadpool_postgres::Pool,
    pub http: reqwest::Client,
}

/// Start a server for a new workspace in which criterion A lists `tests`.
pub async fn world(tests: &[&str]) -> Result<World> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    ensure!(
        url.rsplit('/')
            .next()
            .is_some_and(|db| db.starts_with("joppa_test")),
        "test database required"
    );
    let pool = store::pool(&url)?;
    static MIGRATED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    MIGRATED.get_or_try_init(|| store::migrate(&pool)).await?;
    let ws = format!("record-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Record channel"}),
        json!({"op":"member","id":"alex","role":"human"}),
        json!({"op":"member","id":"another-member","role":"human"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&pool, &ws, "owner", &format!("setup-{i}"), &op).await?;
    }
    store::enroll_service(&pool, &ws, "owner", "bot", "agent").await?;
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    let write = async |actor: &str, command: Value| {
        let id = uuid::Uuid::new_v4().to_string();
        store::execute_remote_input(&pool, &ws, actor, &id, &command).await
    };
    write("owner", json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "requirement":{"title":"Requirements","role":"requirement","root":true,"parents":[]},
        "ac":{"title":"Criteria","role":"ac","parents":["requirement"]}
    }})).await?;
    write("bot", json!({"op":"save_object","kind":"requirement","id":"r","revision":0,"parent":null,"data":{"title":"R","owner":"owner","body":"The rule"},"reason":"Create"})).await?;
    write("bot", json!({"op":"save_object","kind":"ac","id":"a","revision":0,"parent":"requirement:r","data":{"title":"A","owner":"owner"},"reason":"Create"})).await?;
    write(
        "bot",
        json!({"op":"set_ac_tests","target":A,"tests":tests,"reason":"Bind"}),
    )
    .await?;
    let dir = std::env::temp_dir().join(format!("joppa-record-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let token = || uuid::Uuid::new_v4().simple().to_string();
    let (owner, bot, alex, outsider) = (token(), token(), token(), token());
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({ owner.clone(): "owner", bot.clone(): "bot", alex.clone(): "alex", outsider.clone(): "outsider" }).to_string(),
    )?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("JOPPA_DATABASE_URL", &url)
            .env("JOPPA_TOKENS_FILE", &tokens)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .env_remove("DEEPINFRA_API_KEY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    for _ in 0..100 {
        if reqwest::get(format!("{root}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(World {
                _server: server,
                _dir: dir,
                root,
                ws,
                owner,
                bot,
                alex,
                outsider,
                pool,
                http: reqwest::Client::new(),
            });
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}

impl World {
    /// One MCP tool call. Without a token, the call has no session. Gives the HTTP code and body.
    pub async fn mcp_raw(
        &self,
        token: Option<&str>,
        tool: &str,
        args: Value,
    ) -> Result<(u16, Value)> {
        let mut request = self.http.post(format!("{}/mcp", self.root)).json(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}}),
        );
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let reply = request.send().await?;
        Ok((reply.status().as_u16(), reply.json().await?))
    }
    /// One MCP tool call with a token. A refusal gives Err with the exact error text.
    pub async fn mcp(
        &self,
        token: &str,
        tool: &str,
        mut args: Value,
    ) -> Result<Result<Value, String>> {
        args["workspace"] = json!(self.ws);
        let (code, reply) = self.mcp_raw(Some(token), tool, args).await?;
        ensure!(code == 200, "{tool}: {code} {reply}");
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .with_context(|| format!("MCP reply: {reply}"))?;
        Ok(if reply["result"]["isError"] == true {
            Err(text.to_owned())
        } else {
            Ok(serde_json::from_str(text)?)
        })
    }
    /// Record results for one commit through joppa_record_results.
    pub async fn record(
        &self,
        token: &str,
        commit: &str,
        results: Value,
    ) -> Result<Result<Value, String>> {
        self.mcp(
            token,
            "joppa_record_results",
            json!({"commit":commit,"results":results}),
        )
        .await
    }
    /// A record that must succeed.
    pub async fn recorded(&self, commit: &str, results: Value) -> Result<Value> {
        self.record(&self.bot, commit, results)
            .await?
            .map_err(|e| anyhow::anyhow!("record {commit}: {e}"))
    }
    /// One command through joppa_command. A refusal gives Err with the exact error text.
    pub async fn command(&self, token: &str, command: Value) -> Result<Result<Value, String>> {
        let request_id = uuid::Uuid::new_v4().to_string();
        self.mcp(
            token,
            "joppa_command",
            json!({"request_id":request_id,"command":command}),
        )
        .await
    }
    /// The status of A through HTTP: GET /tests?ac=A.
    pub async fn tests_http(&self, token: &str) -> Result<Value> {
        let reply = self
            .http
            .get(format!(
                "{}/api/workspaces/{}/tests?ac={A}",
                self.root, self.ws
            ))
            .bearer_auth(token)
            .send()
            .await?;
        ensure!(
            reply.status().is_success(),
            "GET /tests: {}",
            reply.text().await?
        );
        Ok(reply.json().await?)
    }
    /// The status of A through MCP joppa_tests.
    pub async fn tests_mcp(&self, token: &str) -> Result<Value> {
        self.mcp(token, "joppa_tests", json!({"ac":A}))
            .await?
            .map_err(|e| anyhow::anyhow!("joppa_tests: {e}"))
    }
    /// The journal position that member alex reads through joppa_timeline.
    pub async fn position(&self) -> Result<Value> {
        let page = self
            .mcp(
                &self.alex,
                "joppa_timeline",
                json!({"order":"newest","limit":1}),
            )
            .await?
            .map_err(|e| anyhow::anyhow!("joppa_timeline: {e}"))?;
        ensure!(page["position"].is_u64(), "{page}");
        Ok(page["position"].clone())
    }
    /// The current document of an object through joppa_objects.
    pub async fn object(&self, address: &str) -> Result<Value> {
        self.mcp(&self.bot, "joppa_objects", json!({"address":address}))
            .await?
            .map_err(|e| anyhow::anyhow!("joppa_objects: {e}"))
    }
    /// The database clock that supplies journal timestamps.
    pub async fn clock(&self) -> Result<String> {
        let sql = "SELECT clock_timestamp()::text";
        Ok(self.pool.get().await?.query_one(sql, &[]).await?.get(0))
    }
}

/// The row of one test in a status reply.
pub fn test<'a>(status: &'a Value, address: &str) -> &'a Value {
    status["tests"]
        .as_array()
        .and_then(|tests| tests.iter().find(|t| t["address"] == address))
        .unwrap_or(&Value::Null)
}
