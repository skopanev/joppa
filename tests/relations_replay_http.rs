//! requirement:q438uw0s over a real server: an old journal with links replays into relations
//! with the same state, and a read of an old revision and its history give relations.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::{A, World};
use relations_world::{
    boundaries, edges, link, msg, old_journal, old_save, saved, to, without_key,
};
use serde_json::{Value, json};

/// The criterion boundaries are the time `at` of one old save, for A and its test node only.
async fn boundaries_at(w: &World, at: &str, step: &str) -> Result<()> {
    let found = boundaries(w).await?;
    let keys: Vec<&str> = found.iter().map(|(k, _)| k.as_str()).collect();
    ensure!(
        found.len() == 2 && keys.contains(&A) && keys.iter().any(|k| k.starts_with("test:")),
        "{step}: {found:?}"
    );
    ensure!(
        found.iter().all(|(_, v)| v == at),
        "{step}: {found:?}, not {at}"
    );
    Ok(())
}

// ac:kv34ae3j
// test:d5l51tyo
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_old_journal_replays_into_relations_with_the_same_state() -> Result<()> {
    let mut w = relations_world::world(&["tests/relations_replay_http.rs > a"]).await?;
    for address in ["doc:b", "doc:x", "doc:c", "doc:d"] {
        saved(&w, address, 0, json!({})).await?;
    }
    saved(&w, "doc:d", 1, json!({"body":"Second revision"})).await?;
    w.stop();
    let related = |target: &str| link(target, "related", false, None);
    let d = link("doc:d", "depends_on", true, Some(2));
    let x = link("doc:x", "depends_on", false, None);
    let saves = [
        json!([related("doc:b"), x, related("doc:c"), d.clone()]),
        // Save 2 changes depends_on X to related X in place.
        json!([
            related("doc:b"),
            related("doc:x"),
            related("doc:c"),
            d.clone()
        ]),
        // Save 3 reorders the links.
        json!([related("doc:c"), related("doc:b"), related("doc:x"), d]),
    ];
    let data = json!({"title":"A","owner":"owner"});
    // The older release gives each of these saves a new boundary of A and its test node.
    for (i, links) in saves.into_iter().enumerate() {
        let event = old_save(A, i as u64 + 1, json!("requirement:r"), data.clone(), links);
        let later = 2 - i as i64;
        let written = old_journal(&w, &[event], later).await?;
        joppa::store::backfill_board(&w.pool).await?;
        boundaries_at(&w, &written[0].1, &format!("after save {}", i + 1)).await?;
    }
    joppa::store::check_journal(&w.pool).await?;
    w.start().await?;
    let read = w.object(A).await?;
    ensure!(
        edges(&read, "depends_on") == [("doc:d".to_owned(), Some(2))],
        "{read}"
    );
    let order = [to("doc:c"), to("doc:b"), to("doc:x")];
    ensure!(edges(&read, "related") == order, "{read}");
    Ok(())
}

// ac:bgklhw0n
// test:oiyxzzp8
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_read_of_an_old_revision_and_its_history_give_relations() -> Result<()> {
    let mut w = relations_world::world(&[]).await?;
    for address in ["doc:b", "doc:x", "doc:d"] {
        saved(&w, address, 0, json!({})).await?;
    }
    w.stop();
    // Revision 1 of task:a was stored with links before this change.
    let links = json!([
        link("doc:b", "related", false, None),
        link("doc:x", "depends_on", false, None),
        link("doc:d", "depends_on", true, Some(1))
    ]);
    let data = json!({"title":"a","owner":"owner","body":"Old task"});
    old_journal(&w, &[old_save("task:a", 0, json!(A), data, links)], 0).await?;
    w.start().await?;
    let moved = relations_world::change(&w, "task:a", json!({"parent":"requirement:r"})).await?;
    let old = json!({"address":"task:a","revision":1});
    let old = w.mcp(&w.bot, "joppa_objects", old).await?.map_err(msg)?;
    ensure!(
        edges(&old, "depends_on") == [("doc:d".to_owned(), Some(1))],
        "{old}"
    );
    ensure!(
        edges(&old, "related") == [to("doc:b"), to("doc:x")],
        "{old}"
    );
    ensure!(
        without_key(&old, "links"),
        "revision 1 gives no links: {old}"
    );
    let position = moved["position"].as_i64().unwrap_or(0);
    let page = json!({"after":position - 1,"limit":1});
    let page = w.mcp(&w.bot, "joppa_history", page).await?.map_err(msg)?;
    let entry: &Value = &page["items"][0];
    ensure!(
        entry["position"] == position && entry["change"] == "move",
        "{page}"
    );
    Ok(())
}
