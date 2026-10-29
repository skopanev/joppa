//! The detail shape of MCP object reads (requirement:read-contract). A detail read gives the
//! stored document and the facts that the document does not hold: status, process, comments,
//! titled links, incoming link counts and remote links. It gives no key that repeats another
//! key or the request, and no revision headers: `history: true` gives those.
use crate::read_compact::{page, status, strip_empty, strip_reply};
use anyhow::Result;
use serde_json::{Value, json};

/// Keys that repeat the address, the document or the request.
const COPIES: [&str; 6] = [
    "id",
    "kind",
    "item_id",
    "revision_count",
    "metadata_scope",
    "signal",
];

/// The comments of an annotation map as a list in time order, without duplicate keys.
fn comment_list(comments: &Value) -> Vec<Value> {
    let mut list: Vec<Value> = comments
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, c)| {
            let mut c = c.clone();
            c["id"] = json!(id);
            if c["author"] == c["actor"] {
                c.as_object_mut().map(|m| m.remove("author"));
            }
            if c["created_at"] == c["at"] {
                c.as_object_mut().map(|m| m.remove("created_at"));
            }
            c
        })
        .collect();
    list.sort_by(|a, b| {
        (a["at"].as_str(), a["id"].as_str()).cmp(&(b["at"].as_str(), b["id"].as_str()))
    });
    list
}

/// One titled outgoing link. `document.links` holds the stored link with its pin and
/// blocking flag; this item adds what the target is now. `present` shows only an archived
/// or missing target.
fn link_item(item: &Value) -> Value {
    let mut out = json!({"target": item["link"]["target"], "relation": item["link"]["relation"]});
    for key in ["title", "current_revision"] {
        if !item[key].is_null() {
            out[key] = item[key].clone();
        }
    }
    if item["present"] != true {
        out["present"] = json!(false);
    }
    out
}

/// Remove the keys of `map` that repeat the value of `same`.
fn drop_same(value: &mut Value, keys: &[&str], same: &Value) {
    if let Some(map) = value.as_object_mut() {
        map.retain(|key, v| !(keys.contains(&key.as_str()) && v == same));
    }
}

/// The process facts without copies of the address, the parent, the requirement and its
/// revision. The requirement of a criterion is its document parent.
fn process(reply: &mut Value) {
    let address = reply["address"].clone();
    let parent = reply["document"]["parent"].clone();
    let (current, status) = (
        reply["current_revision"].clone(),
        reply["document"]["data"]["status"].clone(),
    );
    let process = &mut reply["process"];
    // No run gives no verification, and a copy of the revision or of the stored status goes.
    // The requirement revision of a criterion stays only in assessment, which assess reads.
    drop_same(process, &["verification"], &json!("unverified"));
    if process["assessment"].is_object() {
        process.as_object_mut().map(|p| p.remove("revision"));
    }
    drop_same(process, &["revision"], &current);
    drop_same(process, &["status"], &status);
    for entry in process["summary"]["acs"]
        .as_object_mut()
        .into_iter()
        .flatten()
    {
        drop_same(entry.1, &["verification"], &json!("n/a"));
    }
    let (req, revision) = (process["req"].clone(), process["revision"].clone());
    drop_same(
        process,
        &["annotation_address", "criterion", "req"],
        &address,
    );
    drop_same(process, &["req"], &parent);
    drop_same(&mut process["assessment"], &["criterion"], &address);
    drop_same(&mut process["assessment"], &["req"], &req);
    drop_same(&mut process["assessment"], &["revision"], &revision);
    drop_same(&mut process["summary"], &["revision"], &revision);
}

/// The keys that a current and a revision read share: no copies of the address, and the
/// immutable workspace key only when it differs from the workspace name.
fn drop_copies(reply: &mut Value) {
    if let Some(map) = reply.as_object_mut() {
        map.retain(|key, _| !COPIES.contains(&key.as_str()));
    }
    let workspace = reply["workspace"].clone();
    drop_same(reply, &["workspace_key"], &workspace);
}

/// An object without remote links has no remote link list; read it as []. Its revision
/// shows when it is above 0; set_remote_links takes a missing one as 0.
fn drop_empty_remote(reply: &mut Value) {
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

/// A detail reply: the stored document, one page of comments and of titled links, the
/// incoming link counts, one copy of each process fact, and no empty keys outside document.
pub fn detail(
    mut reply: Value,
    comments_cursor: Option<&str>,
    links_cursor: Option<&str>,
) -> Result<Value> {
    let comments = comment_list(&reply["annotations"]["comments"]);
    if let Some(annotations) = reply["annotations"].as_object_mut() {
        annotations.remove("comments");
        // `files` gives the same attachments.
        annotations.remove("attachments");
    }
    let links: Vec<Value> = reply["links"]
        .as_array()
        .into_iter()
        .flatten()
        .map(link_item)
        .collect();
    for (key, entries, cursor) in [
        ("comments", comments, comments_cursor),
        ("links", links, links_cursor),
    ] {
        // An object without comments or links has no such key.
        reply[key] = if entries.is_empty() {
            Value::Null
        } else {
            page(entries, cursor)?
        };
    }
    if let Some(status) = status(&reply) {
        reply["status"] = status;
    }
    if reply["process"]["readiness_blocker"] == reply["process"]["assessment"] {
        reply["process"]
            .as_object_mut()
            .map(|p| p.remove("readiness_blocker"));
    }
    process(&mut reply);
    // `current_revision` is the revision that a write sends. A contract revision shows only
    // when a test list change kept the contract of an older revision.
    let revision = reply["current_revision"].clone();
    drop_same(&mut reply, &["contract_revision"], &revision);
    if let Some(map) = reply.as_object_mut() {
        map.remove("historical");
    }
    drop_copies(&mut reply);
    // `links` gives the outgoing links, so the counts keep the incoming groups.
    if let Some(groups) = reply["link_groups"].as_array_mut() {
        groups.retain(|g| g["direction"] == "incoming");
    }
    if let Some(references) = reply["references"].as_array_mut() {
        for r in references.iter_mut().filter(|r| r["present"] == true) {
            r.as_object_mut().map(|m| m.remove("present"));
        }
    }
    drop_empty_remote(&mut reply);
    // A missing review_required reads as false.
    drop_same(&mut reply, &["review_required"], &json!(false));
    strip_empty(&mut reply);
    Ok(reply)
}

/// A revision read (`revision: N`) or the revision headers (`history: true`), without
/// copies of the address and without empty keys.
pub fn revision(mut reply: Value) -> Value {
    drop_copies(&mut reply);
    drop_empty_remote(&mut reply);
    if let Some(links) = reply["links"].as_array_mut() {
        *links = links.iter().map(link_item).collect();
    }
    strip_reply(reply)
}
