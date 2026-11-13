//! The helpers of the workspace read key tests over a real server (record_world.rs): browser
//! sessions, one HTTP call with a chosen credential, the read-key routes, POST /api/data, the
//! commands route, a server with a kept log, and the purge of each workspace of a test.
#![allow(dead_code)]
use super::record_world::{World, fake_spaces};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::process::{Child, Command, Stdio};

/// One credential or header of a call.
pub enum Auth<'a> {
    Cookie(&'a str),
    Bearer(&'a str),
    Origin(&'a str),
}

/// A browser session of `actor`: the value of the joppa_session cookie.
pub async fn session(w: &World, actor: &str) -> Result<String> {
    let cookie = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    joppa::sessions::create(&w.pool, &cookie, actor, None).await?;
    Ok(cookie)
}

/// One GET or POST of `path` below `root` with the credentials `auth`. Gives the HTTP code and
/// the body. A body that is not JSON comes back as a JSON string.
pub async fn call(
    http: &reqwest::Client,
    root: &str,
    method: &str,
    path: &str,
    auth: &[Auth<'_>],
    body: Option<&Value>,
) -> Result<(u16, Value)> {
    let url = format!("{root}{path}");
    let mut request = match method {
        "GET" => http.get(url),
        _ => http.post(url),
    };
    for a in auth {
        request = match a {
            Auth::Cookie(c) => request.header("Cookie", format!("joppa_session={c}")),
            Auth::Bearer(t) => request.bearer_auth(t),
            Auth::Origin(o) => request.header("Origin", *o),
        };
    }
    if let Some(body) = body {
        request = request.json(body);
    }
    let reply = request.send().await?;
    let code = reply.status().as_u16();
    let text = reply.text().await?;
    Ok((
        code,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    ))
}

/// The path of the read-key route of workspace `ws`, with `/revoke` or "".
pub fn key_path(ws: &str, tail: &str) -> String {
    format!("/api/workspaces/{ws}/read-key{tail}")
}

/// True for a key of the form `jpr_` and 64 lowercase hex characters.
pub fn is_key(key: &str) -> bool {
    key.strip_prefix("jpr_").is_some_and(|hex| {
        hex.len() == 64 && hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
    })
}

/// A new read key of `ws`, from the browser session `cookie` with the site origin.
pub async fn create_key(w: &World, cookie: &str, ws: &str) -> Result<String> {
    let auth = [Auth::Cookie(cookie), Auth::Origin(&w.root)];
    let (code, body) = call(&w.http, &w.root, "POST", &key_path(ws, ""), &auth, None).await?;
    ensure!(code == 200, "POST read-key: {code} {body}");
    let key = body["key"].as_str().context("key")?.to_owned();
    ensure!(is_key(&key), "read key {key:?}");
    Ok(key)
}

/// Revoke the read key of `ws` from the browser session `cookie` with the site origin.
pub async fn revoke_key(w: &World, cookie: &str, ws: &str) -> Result<()> {
    let auth = [Auth::Cookie(cookie), Auth::Origin(&w.root)];
    let path = key_path(ws, "/revoke");
    let (code, body) = call(&w.http, &w.root, "POST", &path, &auth, None).await?;
    ensure!(code == 200, "POST read-key/revoke: {code} {body}");
    Ok(())
}

/// POST /api/data with `body`, and with `Authorization: Bearer <key>` when `key` is set.
pub async fn data(w: &World, key: Option<&str>, body: &Value) -> Result<(u16, Value)> {
    let auth: Vec<Auth> = key.map(Auth::Bearer).into_iter().collect();
    call(&w.http, &w.root, "POST", "/api/data", &auth, Some(body)).await
}

/// POST /api/data that must give 200. Gives the reply.
pub async fn read(w: &World, key: &str, body: Value) -> Result<Value> {
    let (code, reply) = data(w, Some(key), &body).await?;
    ensure!(code == 200, "POST /api/data {body}: {code} {reply}");
    Ok(reply)
}

/// The addresses of the items of a reply, in their order.
pub fn addresses(reply: &Value) -> Vec<String> {
    let items = reply["items"].as_array().into_iter().flatten();
    items
        .filter_map(|i| i["address"].as_str().map(String::from))
        .collect()
}

/// One command through POST /api/workspaces/W/commands with `token`. It must succeed.
pub async fn command(w: &World, token: &str, command: Value) -> Result<Value> {
    let body = json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command});
    let path = format!("/api/workspaces/{}/commands", w.ws);
    let auth = [Auth::Bearer(token)];
    let (code, reply) = call(&w.http, &w.root, "POST", &path, &auth, Some(&body)).await?;
    ensure!(code == 200, "command {}: {code} {reply}", body["command"]);
    Ok(reply)
}

/// Remove each workspace of `ids` and all of its rows.
pub async fn purge(pool: &deadpool_postgres::Pool, ids: &[&str]) -> Result<()> {
    let conn = pool.get().await?;
    for id in ids {
        conn.execute("SELECT joppa.purge_workspace($1)", &[id])
            .await?;
    }
    Ok(())
}

/// A new workspace on its own server: `build` writes its tree, member alex creates a read key
/// in a browser session, and `check` runs with the key. The workspace is purged after.
pub async fn with_key(
    build: impl AsyncFn(&World) -> Result<()>,
    check: impl AsyncFn(&World, &str) -> Result<()>,
) -> Result<()> {
    let w = super::record_world::world(&[]).await?;
    let result = async {
        build(&w).await?;
        let cookie = session(&w, "alex").await?;
        let key = create_key(&w, &cookie, &w.ws).await?;
        check(&w, &key).await
    }
    .await;
    purge(&w.pool, &[&w.ws]).await?;
    result
}

/// The number of rows of joppa.workspace_read_keys for `ws`, and the digest of the row.
pub async fn key_row(w: &World, ws: &str) -> Result<(i64, Option<String>)> {
    let conn = w.pool.get().await?;
    let sql =
        "SELECT count(*), max(token_sha256) FROM joppa.workspace_read_keys WHERE workspace=$1";
    let row = conn.query_one(sql, &[&ws]).await?;
    Ok((row.get(0), row.get(1)))
}

/// A second server on the database of `w` that keeps its stdout and stderr in `log`. A drop
/// stops it.
pub struct Logged {
    child: Child,
    pub root: String,
    pub log: std::path::PathBuf,
}

impl Drop for Logged {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(self.log.parent().unwrap_or(&self.log));
    }
}

/// Start a server with a kept log on the test database. `tokens` maps a token to its actor.
pub async fn logged(tokens: Value) -> Result<Logged> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let dir = std::env::temp_dir().join(format!("joppa-log-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("tokens.json");
    std::fs::write(&file, tokens.to_string())?;
    let log = dir.join("server.log");
    let out = std::fs::File::create(&log)?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let child = Command::new(env!("CARGO_BIN_EXE_joppa"))
        .args(["serve", &port.to_string()])
        .env("JOPPA_DATABASE_URL", &url)
        .env("JOPPA_TOKENS_FILE", &file)
        .env("JOPPA_PUBLIC_URL", &root)
        .env("RUST_LOG", "debug")
        .env_remove("JOPPA_LOGIN_USERS_FILE")
        .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
        .env_remove("JOPPA_ACCESS_RULES_FILE")
        .env("DEEPINFRA_API_KEY", "test-only-key")
        .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
        .envs(fake_spaces::start().env())
        .stdout(Stdio::from(out.try_clone()?))
        .stderr(Stdio::from(out))
        .spawn()?;
    let server = Logged { child, root, log };
    for _ in 0..600 {
        let health = reqwest::get(format!("{}/health", server.root)).await;
        if health.is_ok_and(|r| r.status().is_success()) {
            return Ok(server);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    anyhow::bail!("logged server did not start")
}
