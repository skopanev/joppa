//! The helpers of the computed field tests over a real server (record_world.rs).
#![allow(dead_code)]
use super::record_world::World;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Put the types of `put` with set_tree_schema, as the owner.
pub async fn schema(w: &World, put: Value) -> Result<Result<Value, String>> {
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let tree = tree.map_err(anyhow::Error::msg)?;
    let command = json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
        "put_types":put,"reason":"Compute the union"});
    w.command(&w.owner, command).await
}

/// A node type under `parents` with `extra` keys, such as fields or references.
pub fn node(title: &str, role: &str, parents: &[&str], extra: Value) -> Value {
    let mut t = json!({"title":title,"role":role,"status":"children","root":parents.is_empty(),
        "parents":parents});
    for (k, v) in extra.as_object().into_iter().flatten() {
        t[k] = v.clone();
    }
    t
}

/// The reference sources to the type source, own or with `from`.
pub fn sources(from: Option<&str>) -> Value {
    let mut rule = json!({"target":"source","title":"Sources","multiple":true});
    match from {
        Some(from) => rule["from"] = json!(from),
        None => rule["required"] = json!(true),
    }
    json!({"references":{"sources":rule}})
}

/// Save a new node, or with `revision` a change. `extra` gives parent, relations or data fields.
pub async fn save(
    w: &World,
    address: &str,
    revision: u64,
    extra: Value,
) -> Result<Result<Value, String>> {
    let (kind, id) = address.split_once(':').unwrap_or_default();
    let mut command = json!({"op":"save_object","kind":kind,"id":id,"revision":revision,
        "data":{"title":id,"owner":"owner"},"reason":"Union"});
    for (k, v) in extra.as_object().into_iter().flatten() {
        match k.as_str() {
            "parent" | "relations" => command[k] = v.clone(),
            _ => command["data"][k] = v.clone(),
        }
    }
    w.command(&w.bot, command).await
}

/// Archive a node at its current `revision`.
pub async fn archive(w: &World, address: &str, revision: u64) -> Result<()> {
    let command =
        json!({"op":"archive_object","target":address,"revision":revision,"reason":"Done"});
    w.command(&w.bot, command)
        .await?
        .map_err(anyhow::Error::msg)?;
    Ok(())
}

/// Check the value of `field` of each listed node and the nodes of the data filter `filter`.
pub async fn expect(
    w: &World,
    field: &str,
    wanted: &[(&str, Value)],
    filter: (Value, &[&str]),
) -> Result<()> {
    for (address, value) in wanted {
        let found = w.object(address).await?["data"][field].clone();
        ensure!(&found == value, "{address}: {found} and not {value}");
    }
    let args = json!({"parent":"*","data":{field:filter.0},"limit":25});
    let page = w.mcp(&w.bot, "joppa_objects", args).await?;
    let page = page.map_err(anyhow::Error::msg)?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    let found: BTreeSet<&str> = items.iter().filter_map(|i| i["address"].as_str()).collect();
    let wanted: BTreeSet<&str> = filter.1.iter().copied().collect();
    ensure!(found == wanted, "{}: {found:?}", filter.0);
    Ok(())
}

/// A refusal that contains `error`.
pub fn refused(reply: Result<Value, String>, error: &str) -> Result<()> {
    ensure!(
        reply.as_ref().is_err_and(|e| e.contains(error)),
        "{reply:?}"
    );
    Ok(())
}
