//! The parameters of the MCP read tools, each with a short description, so that an agent
//! sees every option in the parameter schema.
use serde_json::{Value, json};

/// A string parameter with its description.
pub(super) fn text(description: &str) -> Value {
    json!({"type":"string","description":description})
}

/// The text of each cursor parameter.
pub(super) const CURSOR: &str = "next_cursor of the previous page";
/// The text of each workspace parameter.
pub(super) const WORKSPACE: &str = "workspace key or alias";
/// The status tones of the status filter.
const TONES: &str = "tone or list: red, purple, yellow, orange, blue, green";

/// The node filters that joppa_objects and joppa_search take in the same form
/// (src/node_filter.rs). All filters must match. joppa_objects describes them; joppa_search
/// names joppa_objects, so the texts come once.
fn node_filters(mut schema: Value, full: bool) -> Value {
    let one_or_list =
        |d: &str| json!({"type":["string","array"],"items":{"type":"string"},"description":d});
    let filters = json!({
        "parent": text("address or public item ID: its children; * for every level"),
        "depth": {"type":"integer","minimum":1,"maximum":100,"description":"levels below parent, default 1"},
        "kind": one_or_list("type name or list"),
        "status": one_or_list(&format!("{TONES}; purple keeps archived nodes")),
        "owner": text("member ID in data.owner"),
        "assignee": text("member ID or agent alias in data.assignee; \"\" for no assignee"),
        "data": {"type":"object","description":"containment filter on data, e.g. {\"stage\":\"done\"}"},
    });
    for (name, value) in filters.as_object().into_iter().flatten() {
        schema[name] = if full || name == "status" {
            value.clone()
        } else {
            json!({"type":value["type"],"description":"as in joppa_objects"})
        };
    }
    if !full {
        schema["status"]["description"] = json!(TONES);
    }
    schema
}

/// The parameters of joppa_objects, each with a short description.
pub(super) fn objects() -> Value {
    node_filters(
        json!({
            "workspace": text("key or alias; optional with a public item ID"),
            "address": text("1 to 25 comma-separated addresses or public item IDs"),
            "fields": {"type":"array","items":{"type":"string"},
                "description":"keys to return, on a list and on an address read: title, body, status, children, relations, comments, files, child_counts, references or a data name"},
            "limit": {"type":"integer","minimum":1,"maximum":100,
                "description":"items in a page: 1 to 25, or 1 to 100 with fields"},
            "cursor": text(CURSOR),
            "revision": {"type":"integer","minimum":1,"description":"one revision of address"},
            "history": {"type":"boolean","description":"true gives the revision headers"},
            "comments_cursor": text("next_cursor of a comment page"),
            "note_status": {"type":"string","enum":["unprocessed","processed"],"description":"Inbox state"},
            "related_to": text("address: the objects linked to it"),
            "relations_of": text("address: its edges in pages"),
            "direction": {"type":"string","enum":["outgoing","incoming"],"description":"with relations_of"},
            "relation": text("with relations_of: one relation key"),
            "in_relation": text("relation key: its nodes in pages"),
        }),
        true,
    )
}

/// The parameters of joppa_search.
pub(super) fn search() -> Value {
    node_filters(
        json!({
            "workspace": text(WORKSPACE),
            "query": {"type":"string","minLength":1,"maxLength":2000,"description":"words or a copied public ID"},
            "limit": {"type":"integer","minimum":1,"maximum":25,"description":"1 to 25, default 10"},
            "cursor": text(CURSOR),
            "all_versions": {"type":"boolean","description":"also past revisions and archived (purple) objects"},
            "all_workspaces": {"type":"boolean","description":"each workspace that you can read"},
            "fields": {"type":"array","description":"title, excerpt, revision, status"},
        }),
        false,
    )
}
