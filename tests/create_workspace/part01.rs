// Part 1 of tests/create_workspace.rs, included there, so the test names stay unchanged.

struct Server(Child, std::path::PathBuf);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

/// Every row that a creation may change. A refused creation leaves it equal.
async fn snapshot(pool: &deadpool_postgres::Pool) -> Result<Vec<String>> {
    let conn = pool.get().await?;
    let mut rows = vec![];
    for table in [
        "workspaces",
        "workspace_aliases",
        "events",
        "state_entries",
        "board_cache",
        "command_receipts",
    ] {
        rows.push(conn.query_one(
            &format!("SELECT COALESCE(string_agg(t::text, E'\\n' ORDER BY t::text), '') FROM joppa.{table} t"),
            &[],
        ).await?.get(0));
    }
    Ok(rows)
}

/// A browser read of `path` with the session cookie.
fn browser_get(
    client: &reqwest::Client,
    root: &str,
    path: String,
    cookie: &str,
) -> impl std::future::Future<Output = reqwest::Result<reqwest::Response>> + use<> {
    client
        .get(format!("{root}{path}"))
        .header("cookie", cookie)
        .send()
}

/// A browser call that creates a workspace, with an optional cookie and origin.
fn browser_create(
    client: &reqwest::Client,
    root: &str,
    body: Value,
    cookie: Option<&str>,
    origin: Option<&str>,
) -> impl std::future::Future<Output = reqwest::Result<reqwest::Response>> + use<> {
    let mut request = client.post(format!("{root}/api/workspaces")).json(&body);
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    request.send()
}
