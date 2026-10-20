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
            json!({"op":"enroll_service","id":format!("agent-{i}"),"role":"agent"}),
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

    // Every entry after the marked entry 1 is open: at least 31. The first page holds 25,
    // and the cursor gives the rest once, in journal order.
    let first = page(&pool, &ws, json!({"limit": store::HISTORY_LIMIT})).await?;
    let items = first["items"].as_array().cloned().unwrap_or_default();
    ensure!(items.len() == 25, "{first}");
    ensure!(items.iter().all(|i| i["status"] == "open"), "{first}");
    ensure!(items.iter().all(|i| i["at"].is_string()), "{first}");
    // The note row has the status of the note; a member row has no object and no status.
    let note = items.iter().find(|i| i["address"] == "note:intake");
    ensure!(
        note.is_some_and(|n| n["signal"]["tone"].is_string()),
        "{first}"
    );
    let member = items.iter().find(|i| i["op"] == "member");
    ensure!(member.is_some_and(|m| m["signal"].is_null()), "{first}");
    let cursor = first["next_cursor"].as_str().unwrap_or_default().to_owned();
    ensure!(!cursor.is_empty(), "{first}");
    let second = page(
        &pool,
        &ws,
        json!({"limit": store::HISTORY_LIMIT, "cursor": cursor}),
    )
    .await?;
    let rest = second["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        items.len() + rest.len() >= 31 && rest.len() <= 25,
        "{second}"
    );
    let positions: Vec<i64> = items
        .iter()
        .chain(&rest)
        .filter_map(|i| i["position"].as_i64())
        .collect();
    ensure!(
        positions.len() == items.len() + rest.len()
            && positions[0] == 2
            && positions.windows(2).all(|p| p[1] == p[0] + 1),
        "{positions:?}"
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
