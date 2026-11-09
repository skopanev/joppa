//! requirement:item-id-code over a real server: a write takes a public item ID in each field
//! that names an object, and the journal stores the kind:id address.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// The public item ID of one object of the world.
async fn item_id(w: &record_world::World, kind: &str, id: &str) -> Result<String> {
    let sql = "SELECT id FROM joppa.item_ids WHERE workspace=$1 AND kind=$2 AND local_id=$3";
    let row = w
        .pool
        .get()
        .await?
        .query_one(sql, &[&w.ws, &kind, &id])
        .await?;
    Ok(row.get(0))
}

/// The stored commands of the journal after `after`.
async fn stored(w: &record_world::World, after: i64) -> Result<Vec<Value>> {
    let sql = "SELECT command::text FROM joppa.events WHERE workspace=$1 AND position>$2 ORDER BY position";
    let rows = w.pool.get().await?.query(sql, &[&w.ws, &after]).await?;
    rows.iter()
        .map(|r| Ok(serde_json::from_str(r.get(0))?))
        .collect()
}

// ac:qmx0ook5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_write_takes_a_public_item_id_and_the_journal_stores_the_address() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let revision = tree.map_err(anyhow::Error::msg)?["schema"]["revision"].clone();
    let ac = json!({"title":"Criteria","role":"ac","status":"children","parents":["requirement"],
        "references":{"applies_to":{"target":"requirement","title":"Applies to","multiple":true}}});
    let schema = json!({"op":"set_tree_schema","revision":revision,"put_types":{"ac":ac},"reason":"Add a reference"});
    w.command(&w.owner, schema)
        .await?
        .map_err(anyhow::Error::msg)?;
    let other = json!({"op":"save_object","kind":"requirement","id":"other","revision":0,"parent":null,
        "data":{"title":"Other","owner":"owner","body":"Another rule"},"reason":"Create"});
    w.command(&w.bot, other)
        .await?
        .map_err(anyhow::Error::msg)?;
    let (r, a) = (
        item_id(&w, "requirement", "r").await?,
        item_id(&w, "ac", "a").await?,
    );
    let other = item_id(&w, "requirement", "other").await?;
    let before: i64 = w.position().await?.as_i64().context("position")?;
    let operations = json!([
        {"command":{"op":"save_object","kind":"ac","revision":0,"parent":r,
            "data":{"title":"New","owner":"owner","applies_to":[other]},
            "relations":{"related":{"edges":[{"to":a}]}},"reason":"Create by item IDs"}},
        {"command":{"op":"note","target":a,"id":"by-item-id","text":"A comment"}},
        {"command":{"op":"save_object","kind":"ac","id":a,"revision":1,"parent":r,
            "data":{"title":"A renamed","owner":"owner"},"reason":"Edit by item ID"}}
    ]);
    let batch = json!({"request_id":"item-ids-1","operations":operations});
    let reply = w.mcp(&w.bot, "joppa_batch", batch).await?;
    reply.map_err(anyhow::Error::msg)?;
    let second = json!({"op":"note","target":a,"id":"by-owner","text":"A second comment"});
    w.command(&w.owner, second)
        .await?
        .map_err(anyhow::Error::msg)?;
    let commands = stored(&w, before).await?;
    ensure!(commands.len() == 4, "{commands:?}");
    let (new, note, edit, second) = (&commands[0], &commands[1], &commands[2], &commands[3]);
    ensure!(
        new["parent"] == "requirement:r"
            && new["data"]["applies_to"] == json!(["requirement:other"])
            && new["relations"]["related"]["edges"][0]["to"] == "ac:a",
        "{new}"
    );
    ensure!(note["target"] == "ac:a", "{note}");
    ensure!(
        edit["id"] == "a" && edit["parent"] == "requirement:r",
        "{edit}"
    );
    ensure!(second["target"] == "ac:a", "{second}");
    ensure!(w.object("ac:a").await?["data"]["title"] == "A renamed");
    let journal = serde_json::to_string(&commands)?;
    ensure!(
        [&r, &a, &other]
            .iter()
            .all(|id| !journal.contains(id.as_str())),
        "{journal}"
    );
    // A History mark stores the address of each node.
    let position = before + 1;
    let marked = json!({"position":position,"reason":"Done","nodes":[a]});
    let marked = w.mcp(&w.bot, "joppa_history_mark", marked).await?;
    ensure!(marked.map_err(anyhow::Error::msg)?["nodes"] == json!(["ac:a"]));
    let sql = "SELECT nodes::text FROM joppa.history_marks WHERE workspace=$1 AND position=$2";
    let row = w
        .pool
        .get()
        .await?
        .query_one(sql, &[&w.ws, &position])
        .await?;
    ensure!(row.get::<_, String>(0) == r#"["ac:a"]"#);
    let twice = json!({"position":before + 2,"reason":"Done","nodes":[a, "ac:a"]});
    let twice = w.mcp(&w.bot, "joppa_history_mark", twice).await?;
    ensure!(twice.is_err_and(|e| e.contains("a mark names each node once")));
    // An unknown ID, and an ID of another kind in the id of a save, are refused.
    let end = w.position().await?;
    let unknown = json!({"op":"note","target":"nope-R-NOPE0000","id":"x","text":"A comment"});
    let refused = w.command(&w.bot, unknown).await?;
    ensure!(
        refused
            .as_ref()
            .is_err_and(|e| e.contains("unknown object: nope-R-NOPE0000")),
        "{refused:?}"
    );
    let wrong = json!({"op":"save_object","kind":"requirement","id":a,"revision":1,"parent":null,
        "data":{"title":"Wrong","owner":"owner","body":"A rule"},"reason":"Edit"});
    let refused = w.command(&w.bot, wrong).await?;
    ensure!(
        refused
            .as_ref()
            .is_err_and(|e| e.contains("the item ID names an object of type ac, not requirement")),
        "{refused:?}"
    );
    ensure!(w.position().await? == end, "no refused write appends");
    Ok(())
}
