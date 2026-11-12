//! tac:s3ydcg5l and tac:fhr6fazx over a real server: an MCP search runs no statement of the
//! index counters, and an MCP History read loads no object signals. The HTTP routes keep both.
//! pg_stat_statements counts the statements, as tests/read_latency.rs reads it.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/statement_count.rs"]
mod statement_count;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use statement_count::{ONE, install, measure};

/// The body of 600 characters that starts with the word zebra.
fn long_body() -> String {
    let body = format!("zebra{}", " stripe".repeat(85));
    assert_eq!(body.chars().count(), 600);
    body
}

/// One GET of the bot below /api/workspaces/<W>.
async fn get(w: &record_world::World, path: &str) -> Result<Value> {
    let url = format!("{}/api/workspaces/{}{path}", w.root, w.ws);
    let reply = w.http.get(url).bearer_auth(&w.bot).send().await?;
    let code = reply.status();
    let body: Value = reply.json().await?;
    ensure!(code.is_success(), "GET {path}: {code} {body}");
    Ok(body)
}

async fn mcp(w: &record_world::World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args).await?.map_err(anyhow::Error::msg)
}

async fn save(w: &record_world::World, id: &str, body: &str) -> Result<()> {
    let save = json!({"op":"save_object","kind":"requirement","id":id,"revision":0,
        "parent":null,"data":{"title":id,"owner":"owner","body":body},"reason":"Create"});
    w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    Ok(())
}

/// The statements that count the rows of the search index.
const COUNTERS: [&str; 2] = [
    "count(*) from joppa.search_documents",
    "count(*) from joppa.search_revisions",
];

// tac:s3ydcg5l
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn an_mcp_search_runs_no_statement_of_the_index_counters() -> Result<()> {
    let _one = ONE.lock().await;
    let w = record_world::world(&[]).await?;
    install(&w.pool).await?;
    let body = long_body();
    save(&w, "long", &body).await?;
    let tool = async || {
        mcp(&w, "joppa_search", json!({"query":"zebra"})).await?;
        Ok(())
    };
    let found = measure(&w.pool, tool).await?;
    for counter in COUNTERS {
        ensure!(found.naming(&[counter]) == 0, "MCP: {found}");
    }
    let http = async || {
        get(&w, "/search?q=zebra").await?;
        Ok(())
    };
    let found = measure(&w.pool, http).await?;
    ensure!(found.naming(&[COUNTERS[0]]) == 1, "HTTP: {found}");
    // The server cuts the MCP excerpt to 200 characters. HTTP keeps 600 characters.
    let hit = mcp(&w, "joppa_search", json!({"query":"zebra"})).await?;
    let excerpt = hit["items"][0]["excerpt"].as_str().unwrap_or_default();
    ensure!(excerpt.chars().count() == 200, "MCP: {hit}");
    let page = get(&w, "/search?q=zebra").await?;
    let item = &page["items"][0];
    ensure!(
        item["excerpt"].as_str().map(|e| e.chars().count()) == Some(600)
            && item["score"].is_number()
            && page.get("indexed").is_some()
            && page.get("embedded").is_some(),
        "HTTP: {page}"
    );
    Ok(())
}

// tac:fhr6fazx
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn an_mcp_history_read_loads_no_object_signals() -> Result<()> {
    let _one = ONE.lock().await;
    let w = record_world::world(&[]).await?;
    install(&w.pool).await?;
    for i in 0..25 {
        save(&w, &format!("h{i:02}"), "History entry").await?;
    }
    let args = json!({"order":"newest","limit":25});
    let page = mcp(&w, "joppa_history", args.clone()).await?;
    ensure!(page["items"].as_array().map(Vec::len) == Some(25), "{page}");
    let tool = async || {
        mcp(&w, "joppa_history", args.clone()).await?;
        Ok(())
    };
    let found = measure(&w.pool, tool).await?;
    ensure!(
        found.naming(&["signal", "joppa.objects"]) == 0,
        "MCP: {found}"
    );
    let http = async || {
        get(&w, "/history?after=0").await?;
        Ok(())
    };
    let found = measure(&w.pool, http).await?;
    ensure!(
        found.naming(&["coalesce(signal", "from joppa.objects"]) == 1,
        "HTTP: {found}"
    );
    let page = get(&w, "/history?after=0").await?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        !items.is_empty() && items.iter().all(|i| i.get("signal").is_some()),
        "HTTP: {page}"
    );
    Ok(())
}
