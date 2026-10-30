//! The document facts of one object read in either shape: the `document` of an HTTP read, or
//! the flat `revision`, `parent`, `data`, presence and links of an MCP detail read.
use serde_json::{Value, json};

/// The stored document: `document` of an HTTP read, or the MCP detail facts in the stored
/// form. An MCP detail has no actor, time or reason; `history: true` gives those.
pub fn document(read: &Value) -> Value {
    if let Some(document) = read.get("document") {
        return document.clone();
    }
    let links: Vec<Value> = read["links"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| {
            // A link without a relation is related.
            let relation = l.get("relation").cloned().unwrap_or(json!("related"));
            let mut link = json!({"target":l["target"],"relation":relation,
                "revision":l["revision"],"blocking":l["blocking"] == true});
            // An ordering dependency without a pin stores no revision field.
            if relation == "depends_on" && l["revision"].is_null() {
                link.as_object_mut().map(|m| m.remove("revision"));
            }
            link
        })
        .collect();
    json!({"revision":read["revision"],"parent":read["parent"],"data":read["data"],
        "present":read["present"] != false,"links":links})
}

/// The current revision: `current_revision` of an HTTP read, `revision` of an MCP detail.
pub fn current_revision(read: &Value) -> Value {
    match read.get("current_revision") {
        Some(revision) => revision.clone(),
        None => read["revision"].clone(),
    }
}
