//! An old revision still reads as relations after migration 064, over a real server. Revision 1
//! of A is stored before 062 (links only), revision 2 by release 05e8b4a (relations only), and
//! the current rows of A hold the shadow links of 05e8b4a until the next start runs 064.
#[path = "common/older_links.rs"]
mod older_links;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use older_links::{flat, older};
use record_world::World;
use relations_world::{change, link, msg, old_journal, old_save, relations, saved, without_key};
use serde_json::{Value, json};

const A: &str = "doc:a";

/// One revision of A, or the current A without `revision`.
async fn read(w: &World, revision: Option<u64>) -> Result<Value> {
    let mut args = json!({"address":A});
    if let Some(revision) = revision {
        args["revision"] = json!(revision);
    }
    w.mcp(&w.bot, "joppa_objects", args).await?.map_err(msg)
}

/// The History entry at `position`, as the agent bot reads it.
async fn entry(w: &World, position: i64) -> Result<Value> {
    let page = json!({"after":position - 1,"limit":1});
    let page = w.mcp(&w.bot, "joppa_history", page).await?.map_err(msg)?;
    let entry = page["items"][0].clone();
    ensure!(entry["position"] == position, "{page}");
    Ok(entry)
}

/// Rewrite revision 1 of A as the release before 062 stored it, and give the current rows of A
/// the shadow links of release 05e8b4a.
async fn older_rows(w: &World, links: &Value) -> Result<()> {
    let conn = w.pool.get().await?;
    let sql = "SELECT document::text FROM joppa.object_revisions WHERE workspace=$1 AND address=$2 AND revision=1";
    let mut document: Value =
        serde_json::from_str(conn.query_one(sql, &[&w.ws, &A]).await?.get(0))?;
    older(&mut document, links);
    let sql = "UPDATE joppa.object_revisions SET document=$3::text::jsonb WHERE workspace=$1 AND address=$2 AND revision=1";
    conn.execute(sql, &[&w.ws, &A, &document.to_string()])
        .await?;
    let sql = "UPDATE joppa.objects SET document=document||jsonb_build_object('links',joppa.links_shadow(document->'relations')) WHERE workspace=$1 AND address=$2";
    conn.execute(sql, &[&w.ws, &A]).await?;
    let shadow = "jsonb_set(value,'{revisions,0,links}',joppa.links_shadow(value->'revisions'->0->'relations'))";
    let sql = format!(
        "UPDATE joppa.state_entries SET value={shadow},compact=CASE WHEN compact IS NULL THEN NULL ELSE {} END WHERE workspace=$1 AND section='objects' AND key=$2",
        shadow.replace("value", "compact")
    );
    conn.execute(&sql, &[&w.ws, &A]).await?;
    Ok(())
}

// ac:m1cf239d
// test:9sj6s3l4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_old_revision_still_reads_as_relations() -> Result<()> {
    let mut w = relations_world::world(&[]).await?;
    for address in ["doc:b", "doc:c", "doc:d"] {
        saved(&w, address, 0, json!({})).await?;
    }
    // Revision 1 of A: an old save with links.
    w.stop();
    let links = json!([
        link("doc:b", "related", false, None),
        link("doc:c", "depends_on", true, Some(1))
    ]);
    let data = json!({"title":"a","owner":"owner","body":"Old node"});
    let old = [old_save(A, 0, Value::Null, data, links.clone())];
    let first = old_journal(&w, &old, 0).await?[0].0;
    w.start().await?;
    // Revision 2 of A: relations only.
    let related = json!({"related":{"edges":[{"to":"doc:d"}]}});
    let second = change(&w, A, json!({"relations":related})).await?;
    let second = second["position"].as_i64().unwrap_or(0);
    let before = [read(&w, Some(1)).await?, read(&w, Some(2)).await?];
    let history = [entry(&w, first).await?, entry(&w, second).await?];
    w.stop();
    older_rows(&w, &links).await?;
    // The start runs 064. Then a save of A.
    w.start().await?;
    let third = change(&w, A, json!({"body":"Saved after 064"})).await?;
    let third = third["position"].as_i64().unwrap_or(0);

    let one = read(&w, Some(1)).await?;
    let replay_rule = vec![
        ("depends_on".to_owned(), "doc:c".to_owned(), Some(1)),
        ("related".to_owned(), "doc:b".to_owned(), None),
    ];
    let right = flat(&relations(&one)) == replay_rule
        && relations(&one) == relations(&before[0])
        && without_key(&one, "links");
    ensure!(right, "revision 1: {one}");
    let two = read(&w, Some(2)).await?;
    let right = relations(&two) == relations(&before[1]) && without_key(&two, "links");
    ensure!(right, "revision 2: {two}, before {}", before[1]);
    let current = read(&w, None).await?;
    let right = current["revision"] == 3
        && relations(&current) == relations(&before[1])
        && without_key(&current, "links");
    ensure!(right, "current: {current}");
    for (position, earlier) in [first, second].into_iter().zip(&history) {
        let now = entry(&w, position).await?;
        ensure!(
            now == *earlier,
            "history {position}: {now}, before {earlier}"
        );
    }
    let change = entry(&w, third).await?["change"].clone();
    ensure!(change == "save", "history {third}: {change}");
    Ok(())
}
