//! requirement:q438uw0s over a real server: History names a relation change and a flow change,
//! also for an old save that changed only links.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use record_world::World;
use relations_world::{change, declare, link, msg, old_journal, old_save, saved};
use serde_json::{Value, json};

/// The History field change of the entry at `position`, as the agent bot reads it.
async fn change_at(w: &World, position: i64) -> Result<Value> {
    let page = json!({"after":position - 1,"limit":1});
    let page = w.mcp(&w.bot, "joppa_history", page).await?.map_err(msg)?;
    let entry = &page["items"][0];
    ensure!(entry["position"] == position, "{page}");
    Ok(entry["change"].clone())
}

/// The edges of a relation key to `targets`.
fn edges(targets: &[&str]) -> Value {
    json!({"edges":targets.iter().map(|t| json!({"to":t})).collect::<Vec<_>>()})
}

// ac:gzlowv3c
// test:ebqwcot9
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn history_names_a_relation_change_and_a_flow_change() -> Result<()> {
    let mut w = relations_world::world(&[]).await?;
    for address in ["doc:b", "doc:c", "doc:d", "doc:e"] {
        saved(&w, address, 0, json!({})).await?;
    }
    // An old save of doc:l changed only its links.
    w.stop();
    let data = json!({"title":"l","owner":"owner","body":"Old node"});
    let b = link("doc:b", "related", false, None);
    let c = link("doc:c", "related", false, None);
    let old = [
        old_save("doc:l", 0, Value::Null, data.clone(), json!([b.clone()])),
        old_save("doc:l", 1, Value::Null, data, json!([b, c])),
    ];
    let written = old_journal(&w, &old, 0).await?;
    w.start().await?;
    let old_change = change_at(&w, written[1].0).await?;
    ensure!(
        old_change == "relations",
        "the old links save reads {old_change}"
    );
    declare(&w, &[("swap", "Swap")]).await?;
    let first = json!({"related":edges(&["doc:b"]),"swap":edges(&["doc:c"])});
    saved(&w, "doc:a", 0, json!({"relations":first})).await?;
    let steps = [
        (
            json!({"relations":{"related":edges(&["doc:b", "doc:d"])}}),
            "relations",
        ),
        (
            json!({"relations":{"swap":edges(&["doc:c", "doc:e"])}}),
            "flow",
        ),
        (
            json!({"relations":{"related":edges(&["doc:b"]),"swap":edges(&["doc:c"])}}),
            "relations",
        ),
        (
            json!({"body":"New text","relations":{"swap":edges(&["doc:e"])}}),
            "save",
        ),
    ];
    for (extra, wanted) in steps {
        let reply = change(&w, "doc:a", extra.clone()).await?;
        let position = reply["position"].as_i64().unwrap_or(0);
        let found = change_at(&w, position).await?;
        ensure!(found == wanted, "{extra}: change {found}, not {wanted}");
    }
    Ok(())
}
