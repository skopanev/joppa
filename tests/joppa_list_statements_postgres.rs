//! tac:tcospnl8 over a real server: joppa_list gives {id, title} in pages of 25, and
//! hidden: true only for a workspace that the caller hides, from one SQL statement after the
//! token check. pg_stat_statements counts the statements, as tests/read_latency.rs reads it.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/statement_count.rs"]
mod statement_count;
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use statement_count::{ONE, install, measure};

/// A server whose token file names only one new member. Gives the server, its root, the token
/// and the pool.
struct Lister {
    _server: record_world::serve::Server,
    root: String,
    token: String,
    pool: deadpool_postgres::Pool,
    http: reqwest::Client,
}

impl Lister {
    /// One MCP tool call of the member. Gives Err with the text of a refusal.
    async fn call(&self, tool: &str, args: Value) -> Result<Result<Value, String>> {
        let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":tool,"arguments":args}});
        let request = self.http.post(format!("{}/mcp", self.root));
        let reply = request.bearer_auth(&self.token).json(&body).send().await?;
        let reply: Value = reply.error_for_status()?.json().await?;
        let text = reply["result"]["content"][0]["text"].as_str();
        let text = text.with_context(|| format!("MCP reply: {reply}"))?;
        Ok(if reply["result"]["isError"] == true {
            Err(text.to_owned())
        } else {
            Ok(serde_json::from_str(text)?)
        })
    }
}

/// 30 workspaces of the owner with the human member `lister-<n>`, who hides the first one.
/// Gives the server and the 30 workspace IDs.
async fn thirty() -> Result<(Lister, Vec<String>)> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(name.starts_with("joppa_test"), "test database required");
    store::migrate(&pool).await?;
    install(&pool).await?;
    let lister = format!("lister-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let mut ids = Vec::new();
    for i in 0..30 {
        let ws = format!("list-{}", uuid::Uuid::new_v4().simple());
        for (actor, op) in [
            (
                "owner",
                json!({"op":"workspace","title":format!("List {i:02}")}),
            ),
            ("owner", json!({"op":"member","id":lister,"role":"human"})),
        ] {
            let request = uuid::Uuid::new_v4().to_string();
            store::execute_input(&pool, &ws, actor, &request, &op).await?;
        }
        ids.push(ws);
    }
    let hide = json!({"op":"update_workspace","hidden":true});
    let request = uuid::Uuid::new_v4().to_string();
    store::execute_input(&pool, &ids[0], &lister, &request, &hide).await?;
    let dir = std::env::temp_dir().join(format!("joppa-list-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, json!({ token.clone(): lister }).to_string())?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let server = record_world::serve::start_with(&url, &tokens, port, &[]).await?;
    let root = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let lister = Lister {
        _server: server,
        root,
        token,
        pool,
        http,
    };
    Ok((lister, ids))
}

// tac:tcospnl8
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn joppa_list_gives_id_and_title_and_the_own_hidden_flag_from_one_statement() -> Result<()> {
    let _one = ONE.lock().await;
    let (l, ids) = thirty().await?;
    let first = l.call("joppa_list", json!({})).await?;
    let first = first.map_err(anyhow::Error::msg)?;
    let items = first["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        items.len() == 25 && first["total"] == 30 && first["next_cursor"].is_string(),
        "page 1: {first}"
    );
    let rest = json!({"cursor":first["next_cursor"]});
    let rest = l.call("joppa_list", rest).await?;
    let rest = rest.map_err(anyhow::Error::msg)?;
    let mut all = items;
    all.extend(rest["items"].as_array().cloned().unwrap_or_default());
    ensure!(all.len() == 30, "page 2: {rest}");
    let mut seen: Vec<String> = Vec::new();
    for item in &all {
        let id = item["id"].as_str().context("id")?;
        let title = item["title"].clone();
        let wanted = if id == ids[0] {
            json!({"id":id,"title":title,"hidden":true})
        } else {
            json!({"id":id,"title":title})
        };
        ensure!(title.is_string() && *item == wanted, "{item}");
        seen.push(id.to_owned());
    }
    seen.sort();
    let mut expected = ids.clone();
    expected.sort();
    ensure!(seen == expected, "{seen:?}");
    // One joppa_list call adds 1 statement after the token check. A removed tool runs only
    // the token check.
    let check = async || {
        let refused = l.call("joppa_tests", json!({})).await?;
        ensure!(refused.is_err(), "joppa_tests: {refused:?}");
        Ok(())
    };
    let base = measure(&l.pool, check).await?;
    let list = async || {
        l.call("joppa_list", json!({}))
            .await?
            .map_err(anyhow::Error::msg)?;
        Ok(())
    };
    let found = measure(&l.pool, list).await?;
    ensure!(
        found.total() == base.total() + 1,
        "joppa_list: {found}\ntoken check: {base}"
    );
    for table in ["joppa.email_grants", "joppa.domain_grants"] {
        let more = found.naming(&[table]) - base.naming(&[table]);
        ensure!(more == 0, "joppa_list reads {table}: {found}");
    }
    Ok(())
}
