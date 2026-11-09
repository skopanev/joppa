//! Part of `read_detail`: the relation edges, the flat document and the revision reads.
use crate::read_compact::strip_reply;
use serde_json::{Value, json};

/// Keys that repeat the address, the document or the request.
const COPIES: [&str; 8] = [
    "id",
    "kind",
    "item_id",
    "revision_count",
    "metadata_scope",
    "signal",
    "workspace",
    "position",
];

/// The keys of a revision read: the document of that revision and the current revision.
/// Status, process, comments and link counts belong to the current object.
const REVISION_KEYS: [&str; 7] = [
    "address",
    "revision",
    "parent",
    "data",
    "relations",
    "present",
    "current_revision",
];

/// The detail relations: each edge as stored with the title of its target. A pin adds the
/// current target revision, and an archived target `present: false`. save_object takes the
/// relations back as they are. A node without relations has no `relations` key.
pub(super) fn detail_relations(reply: &mut Value) {
    if reply.get("relations").is_none_or(|r| r == &json!({})) {
        reply.as_object_mut().map(|m| m.remove("relations"));
        return;
    }
    let keys = reply["relations"]
        .as_object_mut()
        .into_iter()
        .flat_map(|m| m.values_mut());
    for edge in keys.flat_map(|r| r["edges"].as_array_mut().into_iter().flatten()) {
        if let Some(map) = edge.as_object_mut() {
            if map.get("revision").is_none_or(Value::is_null) {
                map.remove("current_revision");
            }
            if map.get("present") == Some(&json!(true)) {
                map.remove("present");
            }
        }
    }
}

/// The document as top-level keys: revision, parent and data. An archived object shows
/// `present: false`. The actor, time and reason come with `history: true`.
pub(super) fn flatten(reply: &mut Value) {
    let document = reply["document"].take();
    for key in ["revision", "parent", "data"] {
        reply[key] = document[key].clone();
    }
    if document["present"] == false {
        reply["present"] = json!(false);
    }
    reply.as_object_mut().map(|m| m.remove("document"));
}

/// Remove the keys of `map` that repeat the value of `same`.
pub(super) fn drop_same(value: &mut Value, keys: &[&str], same: &Value) {
    if let Some(map) = value.as_object_mut() {
        map.retain(|key, v| !(keys.contains(&key.as_str()) && v == same));
    }
}

/// No copies of the request, of the workspace and of the snapshot position. The address
/// shows only when the request named an alias of it, and the workspace key never.
pub(super) fn drop_copies(reply: &mut Value, asked: &str) {
    if let Some(map) = reply.as_object_mut() {
        map.retain(|key, _| !COPIES.contains(&key.as_str()) && key != "workspace_key");
    }
    drop_same(reply, &["address"], &json!(asked));
}

/// An object without remote links has no remote link list; read it as []. Its revision
/// shows when it is above 0; set_remote_links takes a missing one as 0.
pub(super) fn drop_empty_remote(reply: &mut Value) {
    if reply["remote_links"] == json!([]) && reply["remote_review_required"] != true {
        for key in ["remote_links", "remote_review_required"] {
            reply.as_object_mut().map(|m| m.remove(key));
        }
        if reply["remote_links_revision"] == 0 {
            reply
                .as_object_mut()
                .map(|m| m.remove("remote_links_revision"));
        }
    }
}

/// A revision read (`revision: N`): the revision, parent, data and relations of that
/// document, and the current revision.
pub fn revision(mut reply: Value, asked: &str) -> Value {
    drop_copies(&mut reply, asked);
    drop_empty_remote(&mut reply);
    detail_relations(&mut reply);
    flatten(&mut reply);
    if let Some(map) = reply.as_object_mut() {
        map.retain(|key, _| REVISION_KEYS.contains(&key.as_str()));
    }
    strip_reply(reply)
}

/// The revision headers (`history: true`): no copies of the request and no empty keys.
pub fn headers(mut reply: Value, asked: &str) -> Value {
    drop_copies(&mut reply, asked);
    strip_reply(reply)
}
