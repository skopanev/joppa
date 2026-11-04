//! The parameters of the MCP read tools, each with a short description, so that an agent
//! sees every option in the parameter schema.
use serde_json::{Value, json};

fn text(description: &str) -> Value {
    json!({"type":"string","description":description})
}

/// The node filters that joppa_objects and joppa_search take in the same form
/// (src/node_filter.rs). All filters must match.
fn node_filters(mut schema: Value) -> Value {
    let one_or_list =
        |d: &str| json!({"type":["string","array"],"items":{"type":"string"},"description":d});
    let filters = json!({
        "parent": text("node address or public item ID: every descendant at any depth; * for every level"),
        "depth": {"type":"integer","minimum":1,"maximum":100,
            "description":"levels below the parent, or below the top without a parent; 1 gives the direct children or the roots"},
        "kind": one_or_list("type name or a list of type names"),
        "status": one_or_list("status tone or a list: red, purple, yellow, orange, grey, green; purple keeps the archived nodes, and joppa_search needs all_versions for it"),
        "owner": text("member ID in data.owner"),
        "assignee": text("member ID: data.assignee"),
        "ready": {"type":"boolean","description":"false: the node waits for a dependency, a review or the answer to its assessment question; true: every other node"},
        "data": {"type":"object",
            "description":"JSONB containment filter on document.data, e.g. {\"tags\":[\"kyc\"]}"},
    });
    for (name, value) in filters.as_object().unwrap() {
        schema[name] = value.clone();
    }
    schema
}

/// The parameters of joppa_objects, each with a short description.
pub(super) fn objects() -> Value {
    node_filters(json!({
        "workspace": text("workspace key or alias; optional when a public item ID such as joppa-R-12345678 is given"),
        "address": text("1 to 25 comma-separated kind:id addresses"),
        "fields": {"type":"array","items":{"type":"string"},
            "description":"dotted paths to return (default [\"title\",\"status\"] on lists; address is always included), e.g. [\"title\",\"body\",\"status\",\"children\"]"},
        "limit": {"type":"integer","minimum":1,"maximum":100,
            "description":"items in one list page; more than 25 gives 25"},
        "cursor": text("next_cursor of the previous page"),
        "revision": {"type":"integer","minimum":1,
            "description":"read this one revision of the object at address"},
        "history": {"type":"boolean",
            "description":"true gives the revision headers of the object at address"},
        "comments_cursor": text("next_cursor of the previous comment page of the object"),
        "note_status": {"type":"string","enum":["unprocessed","processed"],
            "description":"Inbox state; the list gives only inputs in this state"},
        "related_to": text("address; the list gives the objects linked to it in either direction"),
        "links_of": text("address; gives its links in pages of 25"),
        "direction": {"type":"string","enum":["outgoing","incoming"],
            "description":"with links_of: only the links in this direction"},
        "relation": text("with links_of: only the links with this relation"),
    }))
}

/// The parameters of joppa_search.
pub(super) fn search() -> Value {
    node_filters(json!({
        "workspace": text("workspace key or alias"),
        "query": {"type":"string","minLength":1,"maxLength":2000,
            "description":"words or a copied public ID; full-text and vector search"},
        "limit": {"type":"integer","minimum":1,"default":10,
            "description":"results in one page; default 10, more than 25 gives 25"},
        "cursor": text("next_cursor of the previous page"),
        "all_versions": {"type":"boolean","default":false,
            "description":"true also reads past revisions and archived objects"},
        "all_workspaces": {"type":"boolean","default":false,
            "description":"true reads each workspace that you can read"},
    }))
}
