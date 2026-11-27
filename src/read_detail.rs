//! The detail shape of MCP object reads (requirement:read-contract): revision, parent, data,
//! status, process, the comment and file counts, the relations with titled edges, incoming
//! edge counts and remote links. `fields` gives comments, files, child counts and references.
//! No key repeats another key or the request; `history: true` gives the revision headers.
use crate::read_compact::{PAGE, page, page_of, status, strip_empty};
use anyhow::Result;
use serde_json::{Value, json};

#[path = "read_links.rs"]
mod links;
use links::{detail_relations, drop_copies, drop_empty_remote, drop_same, flatten};
pub use links::{headers, revision};

/// The keys that a detail read gives only on request with `fields`.
const ON_REQUEST: [&str; 6] = [
    "comments",
    "comments_active",
    "comments_latest",
    "files",
    "references",
    "child_counts",
];

/// The most comments of `comments_latest`.
const LATEST: usize = 5;

/// The comments of an annotation map as a list in time order, without duplicate keys and
/// without `withdrawn: false`.
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
            drop_same(&mut c, &["withdrawn"], &json!(false));
            c
        })
        .collect();
    list.sort_by(|a, b| {
        (a["at"].as_str(), a["id"].as_str()).cmp(&(b["at"].as_str(), b["id"].as_str()))
    });
    list
}

/// The process facts without copies of the address and of the stored status.
fn process(reply: &mut Value) {
    let address = reply["address"].clone();
    let status = reply["document"]["data"]["status"].clone();
    let process = &mut reply["process"];
    drop_same(process, &["annotation_address"], &address);
    drop_same(process, &["status"], &status);
}

/// The comment and file counts. A card read gives them from the store. A full object read
/// (store::read_objects) gives the comments and the files, so the counts come from them.
fn counts(reply: &mut Value) {
    if reply["comments_count"].is_null() && reply["annotations"].is_object() {
        let comments = reply["annotations"]["comments"].as_object().into_iter();
        let active = comments.flatten().filter(|(_, c)| c["withdrawn"] != true);
        reply["comments_count"] = json!(active.count());
    }
    if reply["files_count"].is_null()
        && let Some(files) = reply["files"].as_array()
    {
        reply["files_count"] = json!(files.len());
    }
}

/// The detail keys of an object read with `fields`: revision, parent and data, one page of
/// comments, the short files, the counts (also 0), the relations with titled edges, the
/// incoming edge counts, the references, one copy of each process fact, and no empty keys
/// outside data.
pub fn detail_keys(reply: Value, asked: &str, comments_cursor: Option<&str>) -> Result<Value> {
    detail_keys_sized(reply, asked, (comments_cursor, PAGE))
}

/// `detail_keys` with comment pages of `size` entries, the `limit` of a read of one address.
pub fn detail_keys_sized(
    mut reply: Value,
    asked: &str,
    (comments_cursor, size): (Option<&str>, usize),
) -> Result<Value> {
    counts(&mut reply);
    let comments = comment_list(&reply["annotations"]["comments"]);
    if let Some(annotations) = reply["annotations"].as_object_mut() {
        annotations.remove("comments");
        // `files` gives the same attachments.
        annotations.remove("attachments");
    }
    // A file names no hash and no url: joppa_attachments gives them.
    if let Some(files) = reply["files"].as_array_mut() {
        files
            .iter_mut()
            .for_each(|f| *f = crate::file_list::short(f));
    }
    // The relations as a write sends them back.
    detail_relations(&mut reply);
    // The comments that are not withdrawn: one page of them, and the newest LATEST of them in
    // time order. An object without such comments has neither key.
    let active: Vec<Value> = comments
        .iter()
        .filter(|c| c["withdrawn"] != true)
        .cloned()
        .collect();
    let latest = active[active.len().saturating_sub(LATEST)..].to_vec();
    reply["comments_latest"] = json!(latest);
    reply["comments_active"] = if active.is_empty() {
        Value::Null
    } else {
        page_of(active, comments_cursor, size)?
    };
    // An object without comments has no comments key.
    reply["comments"] = if comments.is_empty() {
        Value::Null
    } else {
        page_of(comments, comments_cursor, size)?
    };
    if let Some(status) = status(&reply) {
        reply["status"] = status;
    }
    process(&mut reply);
    if let Some(map) = reply.as_object_mut() {
        map.remove("historical");
    }
    drop_copies(&mut reply, asked);
    flatten(&mut reply);
    reply.as_object_mut().map(|m| m.remove("current_revision"));
    // `relations` gives the outgoing edges, so the counts keep the incoming groups, and a
    // group names no direction.
    if let Some(groups) = reply["relation_groups"].as_array_mut() {
        groups.retain(|g| g["direction"].is_null() || g["direction"] == "incoming");
        for group in groups.iter_mut().filter_map(Value::as_object_mut) {
            group.remove("direction");
        }
    }
    if let Some(references) = reply["references"].as_array_mut() {
        for r in references.iter_mut().filter(|r| r["present"] == true) {
            r.as_object_mut().map(|m| m.remove("present"));
        }
    }
    drop_empty_remote(&mut reply);
    strip_empty(&mut reply);
    Ok(reply)
}

/// A detail reply (the MCP card): the detail keys without the comments, the files, the
/// references and the child counts. A count shows only when it is above 0.
pub fn detail(reply: Value, asked: &str, comments_cursor: Option<&str>) -> Result<Value> {
    let mut reply = detail_keys(reply, asked, comments_cursor)?;
    if let Some(map) = reply.as_object_mut() {
        map.retain(|key, _| !ON_REQUEST.contains(&key.as_str()));
        // The stamp shows when it exists, and parent_changed only when true (parent_hash.rs).
        map.remove("parent_hash_current");
        map.remove("contract_hash");
    }
    drop_same(&mut reply, &["parent_changed", "blocked"], &json!(false));
    drop_same(&mut reply, &["dependents"], &json!(0));
    drop_same(&mut reply, &["comments_count", "files_count"], &json!(0));
    Ok(reply)
}

/// The detail keys of one list item when `fields` names `LIST_DETAIL_FIELDS`.
pub fn list_detail(item: &Value) -> Value {
    let comments = comment_list(&item["annotations"]["comments"]);
    let active: Vec<Value> = comments
        .iter()
        .filter(|c| c["withdrawn"] != true)
        .cloned()
        .collect();
    let latest = active[active.len().saturating_sub(LATEST)..].to_vec();
    let comments = if comments.is_empty() {
        Value::Null
    } else {
        page(comments, None).unwrap_or(Value::Null)
    };
    let comments_active = if active.is_empty() {
        Value::Null
    } else {
        page(active, None).unwrap_or(Value::Null)
    };
    let mut out = json!({
        "comments": comments,
        "comments_active": comments_active,
        "comments_latest": latest,
        "comments_count": item["comments_count"],
        "files": item["files"],
        "files_count": item["files_count"],
        "references": item["references"],
    });
    if let Some(references) = out["references"].as_array_mut() {
        for r in references.iter_mut().filter(|r| r["present"] == true) {
            r.as_object_mut().map(|m| m.remove("present"));
        }
    }
    strip_empty(&mut out);
    out
}

#[cfg(test)]
#[path = "read_detail_tests.rs"]
mod tests;
