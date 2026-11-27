//! Black-box HTTP, MCP and Postgres tests for Item 2 of SKK 3666:
//! ac:k5xopwlt, ac:ewp7bgsu and ac:5jzkeg7c.
#[path = "common/pg_plan.rs"]
mod pg_plan;
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Context, Result, ensure};
use joppa::embeddings::{Embedder, PROFILE};
use joppa::{node_filter, probe, store};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn db_url() -> Result<(deadpool_postgres::Pool, String)> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = pg_plan::db().await?;
    Ok((pool, url))
}

async fn exec(pool: &deadpool_postgres::Pool, ws: &str, actor: &str, cmd: Value) -> Result<Value> {
    let id = uuid::Uuid::new_v4().to_string();
    store::execute_remote_input(pool, ws, actor, &id, &cmd).await
}

fn serve_with_stderr(
    db: &str,
    endpoint: &str,
    log_path: &std::path::Path,
) -> Result<(std::process::Child, String)> {
    let tokens = std::env::temp_dir().join(format!("joppa-slow-{}.json", uuid::Uuid::new_v4()));
    let map = json!({
        world::token("owner"): "owner",
        world::token("bot"): "bot"
    });
    std::fs::write(&tokens, map.to_string())?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let log_file = std::fs::File::create(log_path)?;
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_joppa"))
        .args(["serve", &port.to_string()])
        .env("JOPPA_DATABASE_URL", db)
        .env("JOPPA_TOKENS_FILE", tokens)
        .env("JOPPA_EMBEDDING_URL", endpoint)
        .env("DEEPINFRA_API_KEY", "test-only-key")
        .envs(world::fake_spaces::start().env())
        .env_remove("JOPPA_LOGIN_USERS_FILE")
        .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
        .env_remove("JOPPA_ACCESS_RULES_FILE")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(log_file))
        .spawn()?;
    Ok((child, format!("http://127.0.0.1:{port}")))
}

struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

include!("speed_fixes_http/part01.rs");
include!("speed_fixes_http/part02.rs");
include!("speed_fixes_http/part03.rs");
