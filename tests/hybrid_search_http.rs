//! requirement:hybrid-search over a real server and a test provider: a query finds an object
//! by meaning, a write wakes the indexer, a provider failure keeps the word results, and the
//! server does not start without the provider key. The tests run one at a time, because
//! each server indexes every pending row of the shared database.
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn db() -> Result<(deadpool_postgres::Pool, String)> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    Ok((pool, url))
}

async fn workspace(pool: &deadpool_postgres::Pool) -> Result<String> {
    let ws = format!("hybrid-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Hybrid"}),
    )
    .await?;
    store::enroll_service(pool, &ws, "owner", "bot", "agent").await?;
    Ok(ws)
}

async fn save(root: &str, ws: &str, id: &str, title: &str, revision: u32) -> Result<Value> {
    world::mcp(root, "owner", "joppa_command", json!({"workspace":ws,"request_id":uuid::Uuid::new_v4().to_string(),
        "command":{"op":"save_object","kind":"domain","id":id,"revision":revision,"parent":null,"data":{"title":title,"owner":"owner"},"reason":"Search test text"}})).await
}

/// Rows without a vector that the indexer can still embed, in one workspace or in all.
async fn pending(pool: &deadpool_postgres::Pool, ws: Option<&str>) -> Result<i64> {
    Ok(pool.get().await?.query_one("SELECT (SELECT count(*) FROM joppa.search_documents WHERE ($1::text IS NULL OR workspace=$1) AND embedding IS NULL AND attempts<5)
        + (SELECT count(*) FROM joppa.search_revisions WHERE ($1::text IS NULL OR workspace=$1) AND embedding IS NULL AND attempts<5)", &[&ws]).await?.get(0))
}

async fn settled(
    pool: &deadpool_postgres::Pool,
    ws: Option<&str>,
    within: Duration,
) -> Result<Duration> {
    let start = Instant::now();
    while start.elapsed() < within {
        if pending(pool, ws).await? == 0 {
            return Ok(start.elapsed());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("rows still wait for a vector after {within:?}")
}

/// The HTTP hit names its kind and ID; the MCP hit names its address.
fn has(reply: &Value, id: &str) -> bool {
    let address = format!("domain:{id}");
    reply["items"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|i| (i["kind"] == "domain" && i["id"] == id) || i["address"] == address.as_str())
    })
}

// hybrid-search-meaning
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_query_finds_an_object_by_meaning_with_no_shared_word() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, url) = db().await?;
    let ws = workspace(&pool).await?;
    let (_fake, endpoint) = world::provider().await?;
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    save(&server.root, &ws, "plans", "Renewal of a paid plan", 0).await?;
    settled(&pool, Some(&ws), Duration::from_secs(10)).await?;
    let http = world::http_search(&server.root, "bot", &ws, &[("q", "extend membership")]).await?;
    let tool = world::mcp(
        &server.root,
        "bot",
        "joppa_search",
        json!({"workspace":ws,"query":"extend membership"}),
    )
    .await?;
    // HTTP names the search mode; MCP gives the hits only.
    ensure!(http["mode"] == "hybrid" && has(&http, "plans"), "{http}");
    ensure!(has(&tool, "plans") && tool.get("mode").is_none(), "{tool}");
    Ok(())
}

// hybrid-search-wake
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_write_wakes_the_indexer_and_the_same_text_keeps_its_vector() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, url) = db().await?;
    let ws = workspace(&pool).await?;
    let (fake, endpoint) = world::provider().await?;
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    // The start drain ends, and then the indexer is idle.
    settled(&pool, None, Duration::from_secs(30)).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let before = fake.calls.load(Ordering::SeqCst);
    save(&server.root, &ws, "wake", "Invoice reminders", 0).await?;
    let waited = settled(&pool, Some(&ws), Duration::from_secs(5)).await?;
    let first = fake.calls.load(Ordering::SeqCst);
    ensure!(first > before, "the write sent no provider request");
    let same = save(&server.root, &ws, "wake", "Invoice reminders", 1).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let second = fake.calls.load(Ordering::SeqCst);
    ensure!(
        second == first,
        "the same text sent {} more requests: {same}",
        second - first
    );
    println!(
        "hybrid-search-wake: vector after {waited:?}, requests {} then {}",
        first - before,
        second - first
    );
    Ok(())
}

// hybrid-search-provider-down
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn a_provider_failure_at_query_time_keeps_the_word_results() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, url) = db().await?;
    let ws = workspace(&pool).await?;
    let (fake, endpoint) = world::provider().await?;
    fake.down.store(true, Ordering::SeqCst);
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    save(&server.root, &ws, "keys", "Passkey sign in", 0).await?;
    let http = world::http_search(&server.root, "bot", &ws, &[("q", "passkey")]).await?;
    let tool = world::mcp(
        &server.root,
        "bot",
        "joppa_search",
        json!({"workspace":ws,"query":"passkey"}),
    )
    .await?;
    ensure!(
        http["semantic_status"] == "unavailable" && has(&http, "keys"),
        "{http}"
    );
    ensure!(
        has(&tool, "keys") && tool.get("semantic_status").is_none(),
        "{tool}"
    );
    Ok(())
}

// hybrid-search-key-required
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn the_server_does_not_start_without_the_provider_key() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (_pool, url) = db().await?;
    let (status, error) = world::serve_without_key(&url)?;
    ensure!(!status.success(), "the server started without the key");
    ensure!(
        error.contains("DEEPINFRA_API_KEY"),
        "the error does not name the variable: {error}"
    );
    Ok(())
}
