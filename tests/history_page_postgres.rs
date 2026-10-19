//! History pages: the board reads 25 entries a page and follows the cursor. Each item has
//! the journal time, the mark of a processed entry, and the current status of its object.
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

async fn page(pool: &deadpool_postgres::Pool, ws: &str, args: Value) -> Result<Value> {
    store::read_history(pool, ws, "owner", &args).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn history_pages_hold_25_entries_with_time_status_and_marks() -> Result<()> {
    let pool = pool().await?;
    let ws = format!("history-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"History pages"}),
    )
    .await?;
    write(&pool, &ws, json!({"op":"save_object","kind":"note","id":"intake","revision":0,"parent":null,"data":{"title":"Intake note","owner":"owner","body":"Refunds are slow"},"links":[],"reason":"Intake"})).await?;
    for i in 0..30 {
        write(
            &pool,
            &ws,
            json!({"op":"member","id":format!("agent-{i}"),"role":"agent"}),
        )
        .await?;
    }
    store::mark_history(
        &pool,
        &ws,
        "owner",
        &json!({"position":1,"reason":"Workspace seen","nodes":[]}),
    )
    .await?;

    // The MCP default is one entry. A limit above 25 is refused.
    let one = page(&pool, &ws, json!({})).await?;
    ensure!(one["items"].as_array().map(Vec::len) == Some(1), "{one}");
    let refused = page(&pool, &ws, json!({"limit": 26})).await.unwrap_err();
    ensure!(
        refused.to_string().contains("between 1 and 25"),
        "{refused}"
    );

    // Open entries: 31 of 32. The first page holds 25, the cursor gives the other 6.
    let first = page(&pool, &ws, json!({"limit": store::HISTORY_LIMIT})).await?;
    let items = first["items"].as_array().cloned().unwrap_or_default();
    ensure!(items.len() == 25, "{first}");
    ensure!(items.iter().all(|i| i["status"] == "open"), "{first}");
    ensure!(items.iter().all(|i| i["at"].is_string()), "{first}");
    let note = &items[0];
    ensure!(note["address"] == "note:intake", "{note}");
    ensure!(note["signal"]["tone"].is_string(), "{note}");
    ensure!(items[1]["signal"].is_null(), "{}", items[1]);
    let cursor = first["next_cursor"].as_str().unwrap_or_default().to_owned();
    ensure!(!cursor.is_empty(), "{first}");
    let second = page(
        &pool,
        &ws,
        json!({"limit": store::HISTORY_LIMIT, "cursor": cursor}),
    )
    .await?;
    ensure!(
        second["items"].as_array().map(Vec::len) == Some(6),
        "{second}"
    );
    ensure!(second["next_cursor"].is_null(), "{second}");

    // With all=true the marked entry comes first with its mark.
    let all = page(&pool, &ws, json!({"limit": 25, "all": true})).await?;
    let marked = &all["items"][0];
    ensure!(
        marked["position"] == 1 && marked["status"] == "processed",
        "{marked}"
    );
    ensure!(
        marked["processed"]["reason"] == "Workspace seen",
        "{marked}"
    );
    ensure!(marked["processed"]["at"].is_string(), "{marked}");
    Ok(())
}
