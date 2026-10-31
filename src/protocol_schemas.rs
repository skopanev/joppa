//! The parameters of the MCP read tools, each with a short description, so that an agent
//! sees every option in the parameter schema.
use serde_json::{Value, json};

fn text(description: &str) -> Value {
    json!({"type":"string","description":description})
}

/// The parameters of joppa_objects, each with a short description.
pub(super) fn objects() -> Value {
    json!({
        "workspace": text("workspace key or alias"),
        "address": text("1 to 25 comma-separated kind:id addresses"),
        "parent": text("direct parent address, * for all objects, or omit for roots; combine with fields to read children in one call"),
        "kind": text("type name; the list gives only objects of this type"),
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
        "work_filter": {"type":"string","enum":["unstarted"],
            "description":"unstarted lists the ready criteria without a pass and without an active Task"},
        "links_of": text("address; gives its links in pages of 25"),
        "direction": {"type":"string","enum":["outgoing","incoming"],
            "description":"with links_of: only the links in this direction"},
        "relation": text("with links_of: only the links with this relation"),
        "data": {"type":"object",
            "description":"JSONB containment filter on document.data, e.g. {\"tags\":[\"kyc\"]}; without parent, searches all objects (*)"},
    })
}

/// The parameters of joppa_tests.
pub(super) fn tests() -> Value {
    json!({
        "workspace": text("workspace key or alias"),
        "ac": text("1 to 100 comma-separated criterion addresses"),
        "parent": text("direct parent address (1 level deep, e.g. requirement:id), or * for all; reads direct child criteria in pages of 100"),
        "kind": text("criterion type with parent; default ac"),
        "task": text("Task address; gives its done state and commit"),
        "summary": {"type":"boolean",
            "description":"true replaces the tests of each criterion with counts per test status"},
        "cursor": text("next_cursor of the previous page"),
    })
}

/// The parameters of joppa_search.
pub(super) fn search() -> Value {
    json!({
        "workspace": text("workspace key or alias"),
        "query": {"type":"string","minLength":1,"maxLength":2000,
            "description":"words or a copied public ID; full-text and vector search"},
        "kind": text("type name; only results of this type"),
        "limit": {"type":"integer","minimum":1,"default":10,
            "description":"results in one page; default 10, more than 25 gives 25"},
        "cursor": text("next_cursor of the previous page"),
        "all_versions": {"type":"boolean","default":false,
            "description":"true also reads past revisions and archived objects"},
        "all_workspaces": {"type":"boolean","default":false,
            "description":"true reads each workspace that you can read"},
    })
}
