//! The document facts of one object read in either shape: the `document` of an HTTP read, or
//! the flat `revision`, `parent`, `data`, presence and relations of an MCP detail read.
#![allow(dead_code)]
use serde_json::{Value, json};

/// A read with each file url cut before its query: a presigned url holds the second of its
/// signature, so two reads in different seconds give different urls for one file.
pub fn unsigned(read: &Value) -> Value {
    let mut read = read.clone();
    for file in read["files"].as_array_mut().into_iter().flatten() {
        if let Some(url) = file["url"]
            .as_str()
            .and_then(|u| u.split('?').next())
            .map(str::to_owned)
        {
            file["url"] = json!(url);
        }
    }
    read
}

/// The stored document: `document` of an HTTP read, or the MCP detail facts in the stored
/// form. An MCP detail has no actor, time or reason; `history: true` gives those.
pub fn document(read: &Value) -> Value {
    if let Some(document) = read.get("document") {
        return document.clone();
    }
    // An MCP edge also gives the title, the current revision and the presence of its target.
    let mut relations = json!({});
    for (key, relation) in read["relations"].as_object().into_iter().flatten() {
        let edges: Vec<Value> = relation["edges"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| {
                let mut edge = json!({"to":e["to"]});
                for field in ["when", "revision"] {
                    if !e[field].is_null() {
                        edge[field] = e[field].clone();
                    }
                }
                edge
            })
            .collect();
        relations[key] = json!({"edges":edges});
    }
    json!({"revision":read["revision"],"parent":read["parent"],"data":read["data"],
        "present":read["present"] != false,"relations":relations})
}

/// The current revision: `current_revision` of an HTTP read, `revision` of an MCP detail.
pub fn current_revision(read: &Value) -> Value {
    match read.get("current_revision") {
        Some(revision) => revision.clone(),
        None => read["revision"].clone(),
    }
}
