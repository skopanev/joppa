//! A requirement names no consumer. A read of its current document, the save examples
//! and the help omit the old `consumers` field. The stored revisions keep it, so no
//! requirement gets a new verification revision and no criterion check resets.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

async fn write(pool: &deadpool_postgres::Pool, ws: &str, command: Value) -> Result<Value> {
    let id = uuid::Uuid::new_v4().to_string();
    store::execute_remote_input(pool, ws, "owner", &id, &command).await
}

async fn read(pool: &deadpool_postgres::Pool, ws: &str, revision: Option<u32>) -> Result<Value> {
    let scope = store::ObjectScope {
        address: Some("requirement:r"),
        revision,
        ..Default::default()
    };
    store::read_objects(pool, ws, "owner", &scope).await
}

// one-consumer-per-criterion-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_requirement_read_shows_no_consumers_and_the_stored_revision_keeps_them() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    ensure!(
        url.rsplit('/')
            .next()
            .is_some_and(|db| db.starts_with("joppa_test")),
        "test database required"
    );
    let pool = store::pool(&url)?;
    store::migrate(&pool).await?;
    let ws = format!("req-consumers-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "setup",
        &json!({"op":"workspace","title":"Requirement consumers"}),
    )
    .await?;
    let save = |kind: &str, id: &str, parent: Option<&str>, revision: u32, data: Value| json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Build the tree"});
    for (kind, id, parent, data) in [
        (
            "consumer",
            "buyer",
            None,
            json!({"title":"Buyer","owner":"owner"}),
        ),
        (
            "domain",
            "d",
            None,
            json!({"title":"Access","owner":"owner"}),
        ),
        (
            "capability",
            "c",
            Some("domain:d"),
            json!({"title":"Keep access","owner":"owner"}),
        ),
    ] {
        write(&pool, &ws, save(kind, id, parent, 0, data)).await?;
    }
    // An older client still sends the consumers of a requirement.
    let data = json!({"title":"Access persists","owner":"owner","body":"Paid access","consumers":["buyer"]});
    write(
        &pool,
        &ws,
        save("requirement", "r", Some("capability:c"), 0, data.clone()),
    )
    .await?;
    let current = read(&pool, &ws, None).await?;
    ensure!(
        current["document"]["data"].get("consumers").is_none(),
        "{current}"
    );
    ensure!(
        current["document"]["data"]["body"] == "Paid access",
        "{current}"
    );
    // The stored revision keeps the field.
    let stored: String = pool.get().await?.query_one("SELECT (document->'data'->'consumers')::text FROM joppa.object_revisions WHERE workspace=$1 AND address='requirement:r' AND revision=1", &[&ws]).await?.get(0);
    ensure!(
        serde_json::from_str::<Value>(&stored)? == json!(["buyer"]),
        "{stored}"
    );
    // A dependency write saves the stored document, so it keeps the field too.
    write(
        &pool,
        &ws,
        save(
            "domain",
            "e",
            None,
            0,
            json!({"title":"Billing","owner":"owner"}),
        ),
    )
    .await?;
    store::save_dependency(
        &pool,
        &ws,
        "owner",
        "dep",
        "requirement:r",
        "domain:e",
        true,
    )
    .await?;
    let stored: String = pool.get().await?.query_one("SELECT (document->'data'->'consumers')::text FROM joppa.object_revisions WHERE workspace=$1 AND address='requirement:r' AND revision=2", &[&ws]).await?.get(0);
    ensure!(
        serde_json::from_str::<Value>(&stored)? == json!(["buyer"]),
        "{stored}"
    );
    let current = read(&pool, &ws, None).await?;
    ensure!(current["current_revision"] == 2, "{current}");
    ensure!(
        current["document"]["data"].get("consumers").is_none(),
        "{current}"
    );
    // A historical read shows the document as the journal saved it.
    let first = read(&pool, &ws, Some(1)).await?;
    ensure!(
        first["document"]["data"]["consumers"] == json!(["buyer"]),
        "{first}"
    );
    // The save example of a requirement has no consumers field.
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    let types: std::collections::BTreeMap<String, joppa::objects::TreeType> =
        serde_json::from_value(tree["schema"]["types"].clone())?;
    let examples = joppa::save_examples::save_examples(&types);
    ensure!(
        examples["requirement"]["data"].get("consumers").is_none(),
        "{examples}"
    );
    ensure!(
        examples["ac"]["data"]["consumer"] == "<consumer:id>",
        "{examples}"
    );
    store::check_journal(&pool).await?;
    Ok(())
}
