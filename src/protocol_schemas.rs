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

fn one_or_list(description: &str) -> Value {
    json!({"type":["string","array"],"items":{"type":"string"},"description":description})
}

/// The node filters that joppa_objects and joppa_search take in the same form
/// (src/node_filter.rs). All filters must match. joppa_objects describes them, and joppa_search
/// names joppa_objects, so the texts come once.
fn node_filters(mut schema: Value, full: bool) -> Value {
    let filters = json!({
        "parent": text("address or public item ID: its children, or * for every level"),
        "depth": {"type":"integer","minimum":1,"maximum":100,"description":"levels below parent, default 1"},
        "kind": one_or_list("type name or list"),
        "status": one_or_list(&format!("{TONES} (purple keeps archived nodes)")),
        "owner": text("member ID in data.owner"),
        "assignee": text("member ID or agent alias, or \"\" for no assignee"),
        "data": {"type":"object","description":"data it contains, e.g. {\"stage\":\"done\"}"},
        "data_any": {"type":"object","description":"{field or a.b: value or list}: any"},
        "data_all": {"type":"object","description":"as data_any: all"},
        "data_none": {"type":"object","description":"as data_any: none"},
    });
    for (name, value) in filters.as_object().into_iter().flatten() {
        // The search repeats the texts of status and assignee: they name values to send.
        schema[name] = if full || name == "status" || name == "assignee" {
            value.clone()
        } else {
            json!({"type":value["type"],"description":"as joppa_objects"})
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
            "workspace": text("key or alias, optional with item ID"),
            "address": text("1 to 100 addresses or item IDs, comma-separated"),
            "fields": {"type":"array","items":{"type":"string"},
                "description":"on a list and on an address read: title, revision, body, status, children, relations, comments, files, child_counts, references, parent_changed, blocked, dependents or a data name"},
            "limit": {"type":"integer","minimum":1,"maximum":100,
                "description":"1 to 25, or 1 to 100 with fields"},
            "cursor": text(CURSOR),
            "revision": {"type":"integer","minimum":1,"description":"one revision of address"},
            "history": {"type":"boolean","description":"revision headers"},
            "comments_cursor": text("next_cursor of a comment page"),
            "note_status": {"type":"string","enum":["unprocessed","processed"],"description":"Inbox state"},
            "related_to": text("address: its linked objects"),
            "relations_of": text("address: its edges in pages"),
            "direction": {"type":"string","enum":["outgoing","incoming"],"description":"with relations_of"},
            "relation": text("with relations_of"),
            "in_relation": text("relation key: its nodes"),
            "count": {"type":"boolean","description":"true: only {total}"},
            "group_by": {"type":"array","description":"1 to 3 of kind, status, owner, assignee, <field>"},
            "not_data": {"type":"object","description":"data it lacks"},
            "has_tests": {"type":"boolean","description":"has test nodes"},
            "comment_prefix": text("active comment ID prefix"),
            "blocked": {"type":"boolean","description":"waits on open work"},
            "order": one_or_list("dependents desc,priority,age"),
            "tiers": {"type":"array","description":"1 to 10 filters, first match first"},
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
            "limit": {"type":"integer","minimum":1,"maximum":25,"description":"default 10"},
            "cursor": text(CURSOR),
            "all_versions": {"type":"boolean","description":"also past revisions and archived objects"},
            "all_workspaces": {"type":"boolean","description":"every readable workspace"},
            "fields": {"type":"array","description":"title, excerpt, revision, status"},
        }),
        false,
    )
}

/// Refuse an argument outside `inputSchema.properties` of `name`. Known removed or server-owned
/// keys pass through so their dedicated error messages stay intact.
pub(super) fn check_arguments(name: &str, a: &Value) -> anyhow::Result<()> {
    let Some(tool) = super::tools::list()
        .into_iter()
        .find(|t| t["name"].as_str() == Some(name))
    else {
        return Ok(());
    };
    let map = match a {
        Value::Null => return Ok(()),
        Value::Object(map) => map,
        _ => anyhow::bail!("arguments must be a JSON object"),
    };
    let props = tool["inputSchema"]["properties"].as_object();
    for key in map.keys() {
        let custom = matches!(key.as_str(), "actor" | "at")
            || matches!(
                (name, key.as_str()),
                ("joppa_objects", "work_filter" | "links_of" | "ready") | ("joppa_search", "ready")
            );
        if !custom && !props.is_some_and(|p| p.contains_key(key)) {
            anyhow::bail!("unknown argument `{key}`");
        }
    }
    Ok(())
}
