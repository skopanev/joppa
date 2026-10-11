//! Private fixture process for Chromium tests against the real server.
use anyhow::{Result, ensure};
use joppa::{sessions, store};
use serde_json::json;
use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Fixture {
    server: Option<Child>,
    dir: std::path::PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(server) = &mut self.server {
            let _ = server.kill();
            let _ = server.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
#[ignore = "started through a private pipe by grey_ac_browser.cjs"]
async fn browser_server() -> Result<()> {
    if std::env::var("JOPPA_BROWSER_HELPER").as_deref() != Ok("1") {
        return Ok(());
    }
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("browser-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "create",
        &json!({"op":"workspace","title":"Browser checks"}),
    )
    .await?;
    let cookie = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&pool, &cookie, "alex").await?;
    let owner = uuid::Uuid::new_v4().simple().to_string();
    let agent = uuid::Uuid::new_v4().simple().to_string();
    let mut fixture = Fixture {
        server: None,
        dir: std::env::temp_dir().join(&ws),
    };
    std::fs::create_dir_all(&fixture.dir)?;
    let tokens = fixture.dir.join("tokens.json");
    let rules = fixture.dir.join("access.json");
    let aliases = fixture.dir.join("users.json");
    std::fs::write(
        &rules,
        json!({"emails":{"alex@example.test":[ws]}}).to_string(),
    )?;
    std::fs::write(&aliases, json!({"alex@example.test":"alex"}).to_string())?;
    std::fs::write(
        &tokens,
        json!({owner.clone():"owner", agent.clone():"bot", uuid::Uuid::new_v4().simple().to_string():"alex"}).to_string(),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [&tokens, &rules, &aliases] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    fixture.server = Some(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("JOPPA_DATABASE_URL", &url)
            .env("JOPPA_TOKENS_FILE", tokens)
            .env("JOPPA_LOGIN_USERS_FILE", &aliases)
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env("JOPPA_ACCESS_RULES_FILE", &rules)
            .env_remove("DEEPINFRA_API_KEY")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let mut ready = false;
    for _ in 0..100 {
        if reqwest::get(format!("{root}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if !ready {
        let server = fixture.server.as_mut().unwrap();
        let _ = server.kill();
        let mut error = String::new();
        if let Some(stderr) = server.stderr.as_mut() {
            stderr.read_to_string(&mut error)?;
        }
        anyhow::bail!("browser server did not start: {error}");
    }
    println!(
        "JOPPA_BROWSER_READY {}",
        json!({"root":root,"ws":ws,"owner":owner,"agent":agent,"cookie":cookie,"access_file":rules})
    );
    std::io::stdout().flush()?;
    tokio::task::spawn_blocking(|| std::io::stdin().read_to_string(&mut String::new())).await??;
    Ok(())
}
