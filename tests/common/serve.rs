//! One `joppa serve` process for the tests that start a real server: the test tokens, the
//! test database, a dead embedding provider and the local Spaces stand-in.
#![allow(dead_code)]
use anyhow::Result;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// The server process. A drop stops it.
pub struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Start a server on `port` with the database `url` and the token file `tokens`, and wait until
/// /health answers.
pub async fn start(url: &str, tokens: &std::path::Path, port: u16) -> Result<Server> {
    start_with(url, tokens, port, &[]).await
}

/// Start a server as `start` does, with the extra variables `env` for the server process.
pub async fn start_with(
    url: &str,
    tokens: &std::path::Path,
    port: u16,
    env: &[(String, String)],
) -> Result<Server> {
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("JOPPA_DATABASE_URL", url)
            .env("JOPPA_TOKENS_FILE", tokens)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .env("DEEPINFRA_API_KEY", "test-only-key")
            .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
            .envs(super::fake_spaces::start().env())
            .envs(env.iter().cloned())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    for _ in 0..100 {
        if reqwest::get(format!("http://127.0.0.1:{port}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(server);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}
