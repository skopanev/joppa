//! requirement:save-examples-show-links: each save example of joppa_help shows a related
//! link, omits id, and gives an inbox type parent null.
#[path = "common/search_world.rs"]
mod world;
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::json;

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

// save-examples-links
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_save_example_shows_a_related_link_and_a_note_has_no_parent() -> Result<()> {
    let (pool, url) = db().await?;
    let ws = format!("examples-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Examples"}),
    )
    .await?;
    store::enroll_service(&pool, &ws, "owner", "bot", "agent").await?;
    let (_fake, endpoint) = world::provider().await?;
    let server = world::serve(&url, &["owner", "bot"], &endpoint).await?;
    let help = world::mcp(&server.root, "bot", "joppa_help", json!({"workspace":ws})).await?;
    let examples = help["examples"].as_object().cloned().unwrap_or_default();
    ensure!(!examples.is_empty(), "no save examples: {help}");
    for (kind, example) in &examples {
        ensure!(
            example["links"] == json!([{"target":"<kind:id>","relation":"related"}]),
            "{kind}: {example}"
        );
        ensure!(example.get("id").is_none(), "{kind} names an id: {example}");
    }
    ensure!(examples["note"]["parent"].is_null(), "{}", examples["note"]);
    Ok(())
}
