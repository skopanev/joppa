//! joppa_objects and joppa_tests without a workspace read the workspace of their public item
//! IDs. The IDs of one call belong to one workspace that the caller can read. A stored
//! 10-character code becomes its first 8 characters, and an old copy still reads the object.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

async fn item(w: &record_world::World, kind: &str, local: &str) -> Result<String> {
    let row = w
        .pool
        .get()
        .await?
        .query_one(
            "SELECT id FROM joppa.item_ids WHERE workspace=$1 AND kind=$2 AND local_id=$3",
            &[&w.ws, &kind, &local],
        )
        .await?;
    Ok(row.get(0))
}

/// The reply of a call without a workspace: whether it is an error, and its text.
async fn bare(
    w: &record_world::World,
    token: &str,
    tool: &str,
    args: Value,
) -> Result<(bool, String)> {
    let (code, reply) = w.mcp_raw(Some(token), tool, args).await?;
    ensure!(code == 200, "{reply}");
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    Ok((reply["result"]["isError"] == true, text.to_owned()))
}

// short-object-ids
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_call_without_a_workspace_reads_the_one_workspace_of_its_ids() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let other = record_world::world(&["tests/y.rs > u"]).await?;
    let (ac, far) = (item(&w, "ac", "a").await?, item(&other, "ac", "a").await?);
    let (error, text) = bare(&w, &w.bot, "joppa_tests", json!({"ac":ac})).await?;
    let tests: Value = serde_json::from_str(&text).unwrap_or_default();
    ensure!(
        !error && tests["tests"][0]["address"] == "tests/x.rs > t",
        "{text}"
    );
    let both = json!({"address":format!("{ac},{far}")});
    let (error, text) = bare(&w, &w.bot, "joppa_objects", both).await?;
    ensure!(
        error && text == "the public item IDs of one call belong to one workspace",
        "{text}"
    );
    let mixed = json!({"address":format!("{ac},nope-R-NOPE0000")});
    let (error, text) = bare(&w, &w.bot, "joppa_objects", mixed).await?;
    ensure!(error && text == "unknown object", "{text}");
    let (error, text) = bare(&w, &w.outsider, "joppa_objects", json!({"address":ac})).await?;
    ensure!(error && text == "unknown object", "{text}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_stored_10_character_code_keeps_its_first_8_characters() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let ac = item(&w, "ac", "a").await?;
    let long = format!("{ac}QZ");
    {
        let conn = w.pool.get().await?;
        conn.batch_execute("ALTER TABLE joppa.item_ids DISABLE TRIGGER item_ids_worm")
            .await?;
        conn.execute("UPDATE joppa.item_ids SET id=$2 WHERE id=$1", &[&ac, &long])
            .await?;
        conn.batch_execute("ALTER TABLE joppa.item_ids ENABLE TRIGGER item_ids_worm")
            .await?;
    }
    joppa::store::migrate(&w.pool).await?;
    ensure!(item(&w, "ac", "a").await? == ac, "the code stays long");
    let (error, text) = bare(&w, &w.bot, "joppa_objects", json!({"address":long})).await?;
    let read: Value = serde_json::from_str(&text).unwrap_or_default();
    ensure!(!error && read["address"] == "ac:a", "{text}");
    Ok(())
}
