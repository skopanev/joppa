//! The detail shape of MCP object reads (requirement:read-contract): revision, parent, data,
//! status, process, comments, one list of titled links, incoming link counts and remote links.
//! No key repeats another key or the request; `history: true` gives the revision headers.
use crate::read_compact::{page, status, strip_empty};
use anyhow::Result;
use serde_json::{Value, json};

#[path = "read_links.rs"]
mod links;
use links::{detail_link, drop_copies, drop_empty_remote, drop_same, flatten};
pub use links::{headers, revision};

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

/// A detail reply: revision, parent and data, one page of comments, all titled links, the
/// incoming link counts, one copy of each process fact, and no empty keys outside data.
pub fn detail(mut reply: Value, asked: &str, comments_cursor: Option<&str>) -> Result<Value> {
    let comments = comment_list(&reply["annotations"]["comments"]);
    if let Some(annotations) = reply["annotations"].as_object_mut() {
        annotations.remove("comments");
        // `files` gives the same attachments.
        annotations.remove("attachments");
    }
    // All outgoing links in one list, as a write sends them back.
    let links: Vec<Value> = reply["links"]
        .as_array()
        .into_iter()
        .flatten()
        .map(detail_link)
        .collect();
    reply["links"] = json!(links);
    // An object without comments has no comments key.
    reply["comments"] = if comments.is_empty() {
        Value::Null
    } else {
        page(comments, comments_cursor)?
    };
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
    drop_copies(&mut reply, asked);
    flatten(&mut reply);
    reply.as_object_mut().map(|m| m.remove("current_revision"));
    drop_same(&mut reply, &["work_ready"], &json!(true));
    drop_same(&mut reply["process"], &["confirmed"], &json!(false));
    drop_same(
        &mut reply["verification_summary"],
        &["total", "passed", "failed"],
        &json!(0),
    );
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
