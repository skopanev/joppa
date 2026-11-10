//! ac:8hgtfw4m over a real server: a relation keeps its facts as addresses (kind:id) in their
//! order, refuses a bad fact, keeps an archived fact that it holds already and gives the same
//! facts after a replay of the journal.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{change, declare, msg, refused, relations, save, saved, stored};
use serde_json::{Value, json};

/// The stored facts of S: F1 and F2 as addresses, in this order.
const FACTS: [&str; 2] = ["doc:f1", "doc:f2"];

/// The MCP read of S and its stored document give swap with the facts [F1, F2].
async fn kept(w: &World, step: &str) -> Result<()> {
    let read = relations(&w.object("doc:s").await?);
    let document = relations(&json!({"document":stored(w, "doc:s").await?.2}));
    for (name, relations) in [("read", &read), ("document", &document)] {
        ensure!(
            relations["swap"]["facts"] == json!(FACTS),
            "{step}: the {name} of S gives {relations}"
        );
    }
    Ok(())
}

/// Archive `address` as the bot at its current revision.
async fn archive(w: &World, address: &str) -> Result<()> {
    let (revision, ..) = stored(w, address).await?;
    let command = json!({"op":"archive_object","target":address,"revision":revision,
        "reason":"Done"});
    w.command(&w.bot, command).await?.map_err(msg)?;
    Ok(())
}

/// The nodes `addresses` (doc:<id>), written through the store in batches of 100.
async fn nodes(w: &World, addresses: &[String]) -> Result<()> {
    for chunk in addresses.chunks(100) {
        let operations: Vec<joppa::store::BatchOperation> = chunk
            .iter()
            .map(|address| {
                let id = address.trim_start_matches("doc:");
                joppa::store::BatchOperation {
                    reference: None,
                    command: json!({"op":"save_object","kind":"doc","id":id,"revision":0,
                        "parent":null,"data":{"title":id,"owner":"owner"},"reason":"Fact target"}),
                }
            })
            .collect();
        let request = uuid::Uuid::new_v4().to_string();
        joppa::store::execute_remote_batch(&w.pool, &w.ws, "bot", &request, &operations).await?;
    }
    Ok(())
}

/// The relations value with swap without edges and with the facts `facts`.
fn facts(facts: Vec<Value>) -> Value {
    json!({"relations":{"swap":{"edges":[],"facts":facts}}})
}

// ac:8hgtfw4m
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn relation_facts_are_kept_as_addresses_and_bad_facts_refused() -> Result<()> {
    let mut w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap")]).await?;
    for address in ["doc:f1", "doc:f2", "doc:x"] {
        saved(&w, address, 0, json!({})).await?;
    }
    archive(&w, "doc:x").await?;
    let others: Vec<String> = (0..201).map(|i| format!("doc:t{i:03}")).collect();
    nodes(&w, &others).await?;
    let sql = "SELECT id FROM joppa.item_ids WHERE workspace=$1 AND kind='doc' AND local_id='f1'";
    let p1: String = w.pool.get().await?.query_one(sql, &[&w.ws]).await?.get(0);
    saved(&w, "doc:s", 0, facts(vec![json!(p1), json!("doc:f2")])).await?;
    kept(&w, "the first save").await?;
    let many: Vec<Value> = others.iter().map(|o| json!(o)).collect();
    for (list, error) in [
        (vec![json!(p1), json!("doc:f1")], "a fact is listed twice"),
        (vec![json!("doc:s")], "a node is not its own fact"),
        (
            vec![json!("doc:x")],
            "linked object is absent from the current tree",
        ),
        (
            vec![json!("note:none")],
            "unknown linked object in this workspace",
        ),
        (many, "a relation holds at most 200 facts"),
    ] {
        let reply = save(&w, "doc:s", 1, facts(list)).await?;
        refused(reply, error)?;
    }
    let revision = stored(&w, "doc:s").await?.0;
    ensure!(revision == 1, "the refusals store no revision: {revision}");
    kept(&w, "after the refusals").await?;
    // F2 is a stored fact of swap, so a save keeps it after its archive.
    archive(&w, "doc:f2").await?;
    change(&w, "doc:s", facts(vec![json!(p1), json!("doc:f2")])).await?;
    kept(&w, "the save after the archive of F2").await?;
    change(&w, "doc:s", json!({"body":"Only new data"})).await?;
    kept(&w, "the save with only new data").await?;
    // The journal of W replays: the server stops, the projections rebuild from the journal,
    // the journal check loads every event, and the server starts again.
    let position = w.position().await?;
    w.stop();
    let stale = "UPDATE joppa.board_cache SET first_view='{}'::jsonb WHERE workspace=$1";
    w.pool.get().await?.execute(stale, &[&w.ws]).await?;
    joppa::store::backfill_board(&w.pool).await?;
    joppa::store::check_journal(&w.pool).await?;
    w.start().await?;
    let after = w.position().await?;
    ensure!(
        after == position,
        "the replay writes no journal entry: {position} then {after}"
    );
    kept(&w, "after the replay").await
}
