//! The fixture of requirement:generic-nodes: requirement:r > ac:a with the given tests, and
//! the type task under ac and requirement, on its own real server.
#![allow(dead_code)]
#[path = "record_world.rs"]
pub mod record_world;
use anyhow::Result;
pub use record_world::World;
use serde_json::{Value, json};

/// The fixture: requirement:r > ac:a with the given tests, and the type task under ac and
/// requirement.
pub async fn world(tests: &[&str]) -> Result<World> {
    let w = record_world::world(tests).await?;
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let revision = tree.map_err(anyhow::Error::msg)?["schema"]["revision"].clone();
    let task =
        json!({"title":"Tasks","role":"task","status":"none","parents":["ac","requirement"]});
    w.command(
        &w.owner,
        json!({"op":"set_tree_schema","revision":revision,
        "put_types":{"task":task},"reason":"Add tasks"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    Ok(w)
}

/// Save a new node as the agent.
pub async fn save(
    w: &World,
    kind: &str,
    id: &str,
    parent: &str,
    data: Value,
    relations: Value,
) -> Result<()> {
    let command = json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,
        "data":data,"relations":relations,"reason":"Plan the work"});
    w.command(&w.bot, command)
        .await?
        .map_err(anyhow::Error::msg)?;
    Ok(())
}

/// The addresses that joppa_objects lists for `filters`.
pub async fn listed(w: &World, filters: Value) -> Result<Vec<String>> {
    let page = w.mcp(&w.bot, "joppa_objects", filters).await?;
    let page = page.map_err(anyhow::Error::msg)?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    Ok(items
        .iter()
        .filter_map(|i| i["address"].as_str().map(String::from))
        .collect())
}
