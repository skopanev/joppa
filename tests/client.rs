use anyhow::{Context, Result, ensure};
use joppa::{commands::Command as Operation, sessions, store};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    time::Duration,
};
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn process(config: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_joppa"));
    c.env_remove("JOPPA_DATABASE_URL")
        .env_remove("JOPPA_ACTOR")
        .env("JOPPA_CONFIG", config);
    c
}
async fn start(url: &str, tokens: &std::path::Path, port: u16) -> Result<Server> {
    let s = Server(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("JOPPA_DATABASE_URL", url)
            .env("JOPPA_TOKENS_FILE", tokens)
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    for _ in 0..100 {
        if reqwest::get(format!("http://127.0.0.1:{port}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(s);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn native_login_mcp_scope_restart_and_logout() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(name.starts_with("joppa_test"), "isolated database required");
    store::migrate(&pool).await?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let ws = format!("client-{suffix}");
    let foreign = format!("private-{suffix}");
    let owner = format!("owner-{suffix}");
    let other = format!("other-{suffix}");
    let make: Operation = serde_json::from_value(json!({"op":"workspace","title":"Client test"}))?;
    store::execute(&pool, &ws, &owner, "create", &make).await?;
    store::execute(&pool, &foreign, &other, "create", &make).await?;
    let dir = std::env::temp_dir().join(format!("joppa-client-{suffix}"));
    std::fs::create_dir_all(&dir)?;
    let tokens = dir.join("tokens.json");
    let config = dir.join("config.json");
    std::fs::write(
        &tokens,
        json!({"test-integration-credential-long-enough":owner}).to_string(),
    )?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let server = start(&url, &tokens, port).await?;
    let base = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let browser = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&pool, &browser, &owner).await?;
    let mut login = Server(
        process(&config)
            .args(["login", "--server", &base])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let mut out = BufReader::new(login.0.stdout.take().unwrap());
    let mut first = String::new();
    out.read_line(&mut first)?;
    let mut second = String::new();
    out.read_line(&mut second)?;
    assert!(first.starts_with(&format!("Open: {base}/connect?code=")));
    let code = second.trim().strip_prefix("Code: ").context("code line")?;
    let approval = json!({"code":code});
    assert_eq!(
        http.post(format!("{base}/auth/device/approve"))
            .header("Origin", &base)
            .json(&approval)
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        http.post(format!("{base}/auth/device/approve"))
            .header("Origin", &base)
            .bearer_auth("test-integration-credential-long-enough")
            .json(&approval)
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        http.post(format!("{base}/auth/device/approve"))
            .header("Origin", "https://attacker.example")
            .header("Cookie", format!("joppa_session={browser}"))
            .json(&approval)
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        http.post(format!("{base}/auth/device/approve"))
            .header("Origin", &base)
            .header("Cookie", format!("joppa_session={browser}"))
            .json(&approval)
            .send()
            .await?
            .status(),
        200
    );
    assert!(login.0.wait()?.success());
    assert!(config.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&config)?.permissions().mode() & 0o777,
            0o600
        );
    }
    let saved: Value = serde_json::from_slice(&std::fs::read(&config)?)?;
    let key = saved["key"].as_str().unwrap();
    assert!(key.starts_with("jp_"));
    assert_eq!(
        http.post(format!("{base}/mcp"))
            .header("Cookie", format!("joppa_session={browser}"))
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .send()
            .await?
            .status(),
        403
    );
    drop(server);
    let _server = start(&url, &tokens, port).await?;
    let me = process(&config).arg("whoami").output()?;
    assert!(me.status.success());
    assert_eq!(serde_json::from_slice::<Value>(&me.stdout)?["actor"], owner);
    let messages = vec![
        json!({"jsonrpc":"2.0","id":0,"method":"server/discover"}),
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"joppa_read","arguments":{"workspace":ws}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"joppa_read","arguments":{"workspace":foreign}}}),
    ];
    let mut child = process(&config)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    {
        let mut input = child.stdin.take().unwrap();
        for message in messages {
            writeln!(input, "{message}")?;
        }
    }
    let response = child.wait_with_output()?;
    assert!(response.status.success());
    let rows: Vec<Value> = String::from_utf8(response.stdout)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0]["error"]["code"], -32601);
    assert_eq!(rows[1]["result"]["serverInfo"]["name"], "joppa");
    assert_eq!(rows[2]["result"]["tools"].as_array().unwrap().len(), 4);
    assert_ne!(rows[3]["result"]["isError"], true);
    assert_eq!(rows[4]["result"]["isError"], true);
    let mut change = process(&config)
        .args(["command", &ws, "via-native-client"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    change.stdin.take().unwrap().write_all(
        br#"{"op":"domain","id":"service","title":"Service","authority":"placeholder"}"#,
    )?;
    assert!(!change.wait_with_output()?.status.success());
    let mut change = process(&config)
        .args(["command", &ws, "via-native-client"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    change.stdin.take().unwrap().write_all(
        json!({"op":"domain","id":"service","title":"Service","authority":owner})
            .to_string()
            .as_bytes(),
    )?;
    assert!(change.wait_with_output()?.status.success());
    assert_eq!(
        store::read(&pool, &ws, &owner).await?["state"]["domains"]["service"]["title"],
        "Service"
    );
    let logout = process(&config).arg("logout").output()?;
    assert!(logout.status.success());
    assert!(!process(&config).arg("whoami").output()?.status.success());
    assert_eq!(
        http.get(format!("{base}/api/me"))
            .bearer_auth(key)
            .send()
            .await?
            .status(),
        401
    );
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
