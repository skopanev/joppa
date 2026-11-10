//! The roles of a production install for the migration tests: `joppa migrate` as the install
//! role, and a serve start as an application role with exactly the grants of server/install.sh.
//! The application role may run no DDL, so a serve start that runs DDL fails.
#![allow(dead_code)]
use super::record_world::{World, serve};
use anyhow::{Context, Result, ensure};
use serde_json::json;

/// One application role of the test database, with its database URL and its token file.
pub struct AppRole {
    role: String,
    url: String,
    tokens: std::path::PathBuf,
}

/// Run `joppa migrate` on the test database as the install role.
pub fn migrate() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_joppa"))
        .arg("migrate")
        .env("JOPPA_DATABASE_URL", &url)
        .output()?;
    let error = String::from_utf8_lossy(&out.stderr);
    ensure!(out.status.success(), "joppa migrate: {error}");
    Ok(())
}

/// The GRANT block of server/install.sh for `role` in database `db`.
fn install_grants(role: &str, db: &str) -> Result<String> {
    let script = include_str!("../../server/install.sh");
    let start = script
        .find("GRANT CONNECT ON DATABASE joppa TO joppa_app;")
        .context("install.sh has no grant block")?;
    let end = start + script[start..].find("\nSQL\n").context("grant block end")?;
    Ok(script[start..end]
        .replace("DATABASE joppa ", &format!("DATABASE \"{db}\" "))
        .replace("joppa_app", role))
}

impl AppRole {
    /// Create a new application role with the grants of server/install.sh and the tokens of W.
    pub async fn create(w: &World) -> Result<Self> {
        let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
        let conn = w.pool.get().await?;
        let db: String = conn
            .query_one("SELECT current_database()", &[])
            .await?
            .get(0);
        let role = format!("joppa_app_{}", uuid::Uuid::new_v4().simple());
        let grants = install_grants(&role, &db)?;
        let setup = format!("CREATE ROLE {role} NOLOGIN; GRANT {role} TO CURRENT_USER; {grants}");
        conn.batch_execute(&setup).await?;
        let separator = if url.contains('?') { '&' } else { '?' };
        let url = format!("{url}{separator}options=-c%20role%3D{role}");
        let name = format!("joppa-app-{}.json", uuid::Uuid::new_v4().simple());
        let tokens = std::env::temp_dir().join(name);
        let names = json!({w.owner.clone():"owner",w.bot.clone():"bot",w.alex.clone():"alex"});
        std::fs::write(&tokens, names.to_string())?;
        Ok(Self { role, url, tokens })
    }

    /// Start the server as this role on the port of W.
    pub async fn start(&self, w: &World) -> Result<serve::Server> {
        let port: u16 = w.root.rsplit(':').next().context("port")?.parse()?;
        let env = [("JOPPA_PUBLIC_URL".to_owned(), w.root.clone())];
        serve::start_with(&self.url, &self.tokens, port, &env).await
    }

    /// Remove the role and its token file.
    pub async fn remove(&self, w: &World) -> Result<()> {
        let role = &self.role;
        let cleanup =
            format!("REVOKE {role} FROM CURRENT_USER; DROP OWNED BY {role}; DROP ROLE {role}");
        w.pool.get().await?.batch_execute(&cleanup).await?;
        let _ = std::fs::remove_file(&self.tokens);
        Ok(())
    }
}
