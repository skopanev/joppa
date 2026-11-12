//! requirement:q438uw0s over a real server: an in_relation read with fields gives only the
//! relation keys that the fields name.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use relations_world::{declare, saved};
use serde_json::{Value, json};

/// The `to` of each edge of the relation `key` of an item.
fn targets(item: &Value, key: &str) -> Vec<String> {
    let edges = item["relations"][key]["edges"]
        .as_array()
        .into_iter()
        .flatten();
    edges
        .map(|e| e["to"].as_str().unwrap_or("").to_owned())
        .collect()
}

/// The keys of an object, sorted.
fn keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

// ac:ig3uv35d
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn in_relation_reads_one_relation_key_with_fields() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap"), ("pay", "Pay")]).await?;
    saved(&w, "doc:n3", 0, json!({})).await?;
    let n2 = json!({"swap":{"edges":[{"to":"doc:n3"}]}});
    saved(&w, "doc:n2", 0, json!({"relations":n2})).await?;
    let n1 = json!({"swap":{"edges":[{"to":"doc:n2"}]},"pay":{"edges":[{"to":"doc:n3"}]}});
    saved(&w, "doc:n1", 0, json!({"relations":n1})).await?;
    let read = async |fields: Value| {
        let args = json!({"in_relation":"swap","fields":fields});
        w.mcp(&w.bot, "joppa_objects", args).await
    };
    let page = read(json!(["title", "relations.swap"])).await?;
    let page = page.map_err(anyhow::Error::msg)?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    let found: Vec<Value> = items.iter().map(|i| i["address"].clone()).collect();
    ensure!(found == [json!("doc:n1"), json!("doc:n2")], "{page}");
    for (item, to) in items.iter().zip(["doc:n2", "doc:n3"]) {
        ensure!(
            keys(item) == ["address", "relations", "title"]
                && keys(&item["relations"]) == ["swap"]
                && targets(item, "swap") == [to],
            "{item}"
        );
    }
    // fields ["relations"] gives every relation key of each node.
    let all = read(json!(["relations"])).await?;
    let all = all.map_err(anyhow::Error::msg)?;
    let first = &all["items"][0];
    ensure!(
        first["address"] == "doc:n1"
            && targets(first, "swap") == ["doc:n2"]
            && targets(first, "pay") == ["doc:n3"],
        "{all}"
    );
    let refused = read(json!(["title", "relations.nope"])).await?;
    ensure!(
        refused
            .as_ref()
            .is_err_and(|e| e.contains("unknown link relation")),
        "{refused:?}"
    );
    Ok(())
}
