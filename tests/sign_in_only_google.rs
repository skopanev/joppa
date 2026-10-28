//! The real server offers no password or other-provider sign-in endpoint.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::json;
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Server(Child, std::path::PathBuf);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn password_and_github_sign_in_endpoints_do_not_exist() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        db.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let dir = std::env::temp_dir().join(format!("joppa-sign-in-{id}"));
    std::fs::create_dir_all(&dir)?;
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({format!("owner-key-{id}"):"owner"}).to_string(),
    )?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("JOPPA_DATABASE_URL", &url)
            .env("JOPPA_TOKENS_FILE", &tokens)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env("DEEPINFRA_API_KEY", "test-only-key")
            .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
        dir,
    );
    let root = format!("http://127.0.0.1:{port}");
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
    ensure!(ready, "test server did not start");
    // Requests without a session reach the full router of the server.
    let client = reqwest::Client::new();
    let password = client
        .post(format!("{root}/auth/password"))
        .header("origin", &root)
        .json(&json!({"email":"maria@example.com","password":"secret"}))
        .send()
        .await?;
    assert_eq!(password.status(), 404, "POST /auth/password");
    let github = client.get(format!("{root}/auth/github")).send().await?;
    assert_eq!(github.status(), 404, "GET /auth/github");
    // The Google sign-in route still exists; it refuses a request without an origin.
    let google = client.post(format!("{root}/auth/login")).send().await?;
    assert_eq!(google.status(), 403, "POST /auth/login");
    Ok(())
}
