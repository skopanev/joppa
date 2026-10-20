//! History marks and reads: the default read, all=true, a mark with its server fields, and
//! the marks that Joppa refuses. Each test uses its own workspace.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

async fn pool() -> Result<deadpool_postgres::Pool> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
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
    Ok(pool)
}

async fn write(pool: &deadpool_postgres::Pool, ws: &str, command: Value) -> Result<Value> {
    store::execute_remote_input(
        pool,
        ws,
        "owner",
        &uuid::Uuid::new_v4().to_string(),
        &command,
    )
    .await
}

async fn history(pool: &deadpool_postgres::Pool, ws: &str, args: Value) -> Result<Value> {
    store::read_history(pool, ws, "bot", &args).await
}

async fn mark(pool: &deadpool_postgres::Pool, ws: &str, args: Value) -> Result<Value> {
    store::mark_history(pool, ws, "bot", &args).await
}

/// The last journal position of the workspace.
async fn head(pool: &deadpool_postgres::Pool, ws: &str) -> Result<i64> {
    Ok(pool
        .get()
        .await?
        .query_one(
            "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0))
}

/// A workspace with member bot, a root Task type and `tasks` Tasks t1, t2 and so on.
async fn workspace(pool: &deadpool_postgres::Pool, tasks: usize) -> Result<String> {
    let ws = format!("marks-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"History marks"}),
    )
    .await?;
    write(pool, &ws, json!({"op":"member","id":"bot","role":"agent"})).await?;
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    write(pool, &ws, json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Use root Tasks","types":{
        "task":{"title":"Tasks","role":"task","root":true,"parents":["*"]}
    }})).await?;
    for i in 1..=tasks {
        write(pool, &ws, json!({"op":"save_object","kind":"task","id":format!("t{i}"),"revision":0,"parent":null,"data":{"title":format!("Task {i}"),"owner":"owner","contract":"Implement the agreed part","purpose":"implementation","projects":[]},"reason":"Create work"})).await?;
    }
    Ok(ws)
}

fn items(page: &Value) -> Vec<Value> {
    page["items"].as_array().cloned().unwrap_or_default()
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_default_read_returns_the_oldest_open_entry() -> Result<()> {
    // ac:37bb64b0: entry 1 is processed, entries 2 and 3 are open.
    let pool = pool().await?;
    let ws = workspace(&pool, 1).await?;
    mark(&pool, &ws, json!({"position":1,"reason":"Seen","nodes":[]})).await?;
    let page = history(&pool, &ws, json!({})).await?;
    let list = items(&page);
    ensure!(list.len() == 1, "{page}");
    ensure!(
        list[0]["position"] == 2 && list[0]["status"] == "open",
        "{page}"
    );
    ensure!(page["next_item"] == 2, "{page}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn all_true_also_returns_processed_entries_in_journal_order() -> Result<()> {
    // ac:8efae8ce: entry 1 is processed and entry 2 is open.
    let pool = pool().await?;
    let ws = workspace(&pool, 1).await?;
    mark(&pool, &ws, json!({"position":1,"reason":"Seen","nodes":[]})).await?;
    let page = history(&pool, &ws, json!({"all": true, "limit": 15})).await?;
    let list = items(&page);
    ensure!(
        list[0]["position"] == 1 && list[0]["status"] == "processed",
        "{page}"
    );
    ensure!(
        list[1]["position"] == 2 && list[1]["status"] == "open",
        "{page}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_mark_records_actor_time_reason_and_nodes_without_a_journal_entry() -> Result<()> {
    // ac:97e320e8: bot marks entry 2 with Tasks T1 and T2.
    let pool = pool().await?;
    let ws = workspace(&pool, 2).await?;
    let conn = pool.get().await?;
    let clock = "SELECT clock_timestamp()::text";
    let before_head = head(&pool, &ws).await?;
    let before: String = conn.query_one(clock, &[]).await?.get(0);
    let reply = mark(
        &pool,
        &ws,
        json!({"position":2,"reason":"Cut into two Tasks","nodes":["task:t1","task:t2"]}),
    )
    .await?;
    let after: String = conn.query_one(clock, &[]).await?.get(0);
    ensure!(
        head(&pool, &ws).await? == before_head,
        "a mark adds no journal entry"
    );
    let at = reply["at"].as_str().unwrap_or_default().to_owned();
    let inside: bool = conn
        .query_one(
            "SELECT $1::text::timestamptz BETWEEN $2::text::timestamptz AND $3::text::timestamptz",
            &[&at, &before, &after],
        )
        .await?
        .get(0);
    ensure!(inside, "{before} <= {at} <= {after}");
    let page = history(&pool, &ws, json!({"all": true, "limit": 5})).await?;
    let entry = items(&page)
        .into_iter()
        .find(|i| i["position"] == 2)
        .unwrap_or_default();
    ensure!(entry["status"] == "processed", "{entry}");
    ensure!(
        entry["processed"]
            == json!({"actor":"bot","at":at,"reason":"Cut into two Tasks","nodes":["task:t1","task:t2"]}),
        "{entry}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_mark_refuses_16_nodes_a_second_mark_and_server_fields() -> Result<()> {
    // ac:18f58d7b: entry 2 is processed, entry 3 is open, and 16 Tasks exist.
    let pool = pool().await?;
    let ws = workspace(&pool, 16).await?;
    mark(
        &pool,
        &ws,
        json!({"position":2,"reason":"First mark","nodes":["task:t1"]}),
    )
    .await?;
    let all: Vec<String> = (1..=16).map(|i| format!("task:t{i}")).collect();
    let refusals = [
        (
            json!({"position":3,"reason":"Too many","nodes":all}),
            "at most 15 nodes",
        ),
        (
            json!({"position":2,"reason":"Again","nodes":["task:t2"]}),
            "already processed",
        ),
        (
            json!({"position":3,"reason":"Own actor","actor":"eve"}),
            "actor comes from the server",
        ),
        (
            json!({"position":3,"reason":"Own time","at":"2020-01-01T00:00:00Z"}),
            "at comes from the server",
        ),
    ];
    for (args, error) in refusals {
        let e = mark(&pool, &ws, args.clone())
            .await
            .expect_err("the mark must be refused");
        ensure!(e.to_string().contains(error), "{args}: {e}");
    }
    let page = history(&pool, &ws, json!({"all": true, "limit": 5})).await?;
    let list = items(&page);
    let entry = |p: i64| list.iter().find(|i| i["position"] == p).cloned();
    ensure!(entry(3).is_some_and(|e| e["status"] == "open"), "{page}");
    ensure!(
        entry(2).is_some_and(|e| e["processed"]["reason"] == "First mark"
            && e["processed"]["nodes"] == json!(["task:t1"])),
        "{page}"
    );
    Ok(())
}
