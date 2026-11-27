//! The workspace of an MCP call that omits it: the workspace of a write key, the workspace of
//! a public item ID, else the one workspace that the caller reads. A caller with more
//! workspaces gets their IDs to choose from.
use deadpool_postgres::Pool;
use joppa::store;
use serde_json::{Value, json};

/// The tools that read or write one workspace.
const TOOLS: [&str; 11] = [
    "joppa_objects",
    "joppa_search",
    "joppa_tree",
    "joppa_attach",
    "joppa_attachments",
    "joppa_record_results",
    "joppa_history",
    "joppa_history_mark",
    "joppa_command",
    "joppa_batch",
    "joppa_help",
];

/// The keys that can hold a public item ID, which names its own workspace.
const ID_KEYS: [&str; 3] = ["address", "parent", "target"];

/// The arguments with a workspace, or None when the call has one or needs none.
pub(super) async fn with_workspace(
    pool: &Pool,
    actor: &str,
    name: &str,
    a: &Value,
) -> anyhow::Result<Option<Value>> {
    let sent = a["workspace"].as_str().is_some_and(|w| !w.is_empty());
    let none_needed = (name == "joppa_search" && a["all_workspaces"] == true)
        || (name == "joppa_help" && a["section"] != "examples");
    if sent || none_needed || !TOOLS.contains(&name) {
        return Ok(None);
    }
    let mut out = a.clone();
    if let Some(ws) = joppa::write_key::current() {
        out["workspace"] = json!(ws);
        return Ok(Some(out));
    }
    let ids: Vec<String> = ID_KEYS
        .iter()
        .filter_map(|key| a[*key].as_str())
        .flat_map(|text| text.split(',').map(str::trim))
        .filter(|key| !key.is_empty() && !key.contains(':') && *key != "*")
        .map(str::to_owned)
        .collect();
    out["workspace"] = match ids.is_empty() {
        false => json!(store::item_workspace(pool, actor, &ids).await?),
        true => json!(only_workspace(pool, actor).await?),
    };
    Ok(Some(out))
}

/// The one workspace that `actor` reads, else a refusal that names the choices.
async fn only_workspace(pool: &Pool, actor: &str) -> anyhow::Result<String> {
    let listed = store::list(pool, actor).await?;
    let mut ids: Vec<String> = listed
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| w["id"].as_str().map(str::to_owned))
        .collect();
    ids.sort();
    match ids.len() {
        0 => anyhow::bail!("missing workspace"),
        1 => Ok(ids.remove(0)),
        _ => anyhow::bail!("missing workspace: choose one of {}", ids.join(", ")),
    }
}
