//! The local relations of a node. A save, a stored document, a journal command, a list item
//! and a detail read all hold `relations: {<key>: {edges: [{to, ...}]}}`.
#![allow(dead_code)]
use serde_json::{Value, json};

/// A relations value for a save: one key with an edge to each target.
pub fn one(key: &str, targets: &[&str]) -> Value {
    let edges: Vec<Value> = targets.iter().map(|t| json!({"to": t})).collect();
    let mut relations = json!({});
    relations[key] = json!({"edges": edges});
    relations
}

/// A relations value for a save: one depends_on edge to `target`, with a pin when given.
pub fn depends_on(target: &str, revision: Option<u64>) -> Value {
    let mut edge = json!({"to": target});
    if let Some(revision) = revision {
        edge["revision"] = json!(revision);
    }
    json!({"depends_on": {"edges": [edge]}})
}

/// The edges of `key` in `node["relations"]`. A missing key gives none.
pub fn edges(node: &Value, key: &str) -> Vec<Value> {
    node["relations"][key]["edges"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// The `to` of each edge of `key`.
pub fn targets(node: &Value, key: &str) -> Vec<String> {
    edges(node, key)
        .iter()
        .filter_map(|e| e["to"].as_str().map(str::to_owned))
        .collect()
}

/// Every edge as (key, to), in key order and then in edge order.
pub fn pairs(node: &Value) -> Vec<(String, String)> {
    let mut out = vec![];
    for (key, relation) in node["relations"].as_object().into_iter().flatten() {
        for edge in relation["edges"].as_array().into_iter().flatten() {
            let to = edge["to"].as_str().unwrap_or_default();
            out.push((key.clone(), to.to_owned()));
        }
    }
    out
}

/// The stored relations of a model revision, in the form of its JSON document.
pub fn of<T: serde::Serialize>(revision: &T) -> Value {
    serde_json::to_value(revision).expect("a revision serializes")
}
