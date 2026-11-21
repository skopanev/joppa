//! The write key of a workspace (`jpt_<64hex>`) and its MCP checks against the tree schema.
//! The key reads only its own workspace, calls joppa_record_results when test_results.key_write
//! is true, and updates only fields with key_write true on present objects.
use crate::model::Workspace;
use crate::objects::{address, resolve_address, schema};
use anyhow::{Result, bail, ensure};
use serde_json::Value;

/// The start of each write key. The rest is 64 lowercase hex characters.
pub const KEY_PREFIX: &str = "jpt_";

tokio::task_local! {
    static WORKSPACE: Option<String>;
}

/// True when `token` has the form of a workspace write key.
pub fn is_write_key(token: &str) -> bool {
    token.strip_prefix(KEY_PREFIX).is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Run `work` scoped to `workspace` when the call authenticated with a workspace write key.
pub async fn scope<F: std::future::Future>(workspace: Option<String>, work: F) -> F::Output {
    WORKSPACE.scope(workspace, work).await
}

/// The canonical workspace ID of the current write-key call, or None for any other credential.
pub fn current() -> Option<String> {
    WORKSPACE.try_with(Clone::clone).ok().flatten()
}

/// Refuse an MCP tool or cross-workspace search flag that a workspace write key may not call.
pub fn check_tool(name: &str, args: &Value) -> Result<()> {
    if current().is_none() {
        return Ok(());
    }
    match name {
        "joppa_objects"
        | "joppa_tree"
        | "joppa_history"
        | "joppa_attachments"
        | "joppa_help"
        | "joppa_record_results"
        | "joppa_command"
        | "joppa_batch"
        | "joppa_tasks"
        | "joppa_timeline"
        | "joppa_tests" => Ok(()),
        "joppa_search" => {
            ensure!(
                args["all_workspaces"] != true,
                "a workspace write key reads only its own workspace"
            );
            Ok(())
        }
        _ => bail!("a workspace write key cannot call {name}"),
    }
}

/// Refuse an operation that changes anything outside key_write fields of a present object.
pub fn check_operation(w: &Workspace, actor: &str, value: &Value, clean: bool) -> Result<()> {
    let Some(_) = current() else {
        return Ok(());
    };
    ensure!(
        crate::commands::is_human_member(w, actor),
        crate::commands::HUMAN_MEMBER_REQUIRED
    );
    ensure!(
        value["op"] == "save_object",
        "a workspace write key may only update key_write fields with save_object"
    );
    ensure!(!clean, "a workspace write key cannot use clean");
    ensure!(
        value["revision"].as_u64().unwrap_or(0) > 0,
        "a workspace write key updates only a present object"
    );
    ensure!(
        value.get("relations").is_none() && value.get("links").is_none(),
        "a workspace write key cannot change relations"
    );
    let (Some(kind), Some(id)) = (value["kind"].as_str(), value["id"].as_str()) else {
        bail!("save_object needs kind and id");
    };
    let addr = address(kind, id);
    let key = resolve_address(w, &addr);
    let Some(object) = w
        .objects
        .get(key)
        .filter(|o| o.current().present && o.kind == kind)
    else {
        bail!("a workspace write key updates only a present object");
    };
    if let Some(sent_parent) = value.get("parent") {
        let wanted = match sent_parent {
            Value::Null => None,
            Value::String(p) => Some(resolve_address(w, p)),
            _ => bail!("invalid parent"),
        };
        let stored = object
            .current()
            .parent
            .as_deref()
            .map(|p| resolve_address(w, p));
        ensure!(
            wanted == stored,
            "a workspace write key cannot change parent"
        );
    }
    let Some(sent) = value.get("data").and_then(Value::as_object) else {
        bail!("save_object data must be an object");
    };
    ensure!(
        !sent.is_empty(),
        "save_object needs at least one key_write field"
    );
    ensure!(
        !sent.contains_key("workspace"),
        "a workspace write key cannot change workspace"
    );
    let tree = schema(w);
    let fields = tree.types.get(&object.kind).map(|t| &t.fields);
    for field in sent.keys() {
        let allowed = fields
            .and_then(|f| f.get(field))
            .is_some_and(|r| r.key_write && r.source() == "own");
        ensure!(
            allowed,
            "field {field} of {} does not allow key_write in the tree schema",
            object.kind
        );
    }
    Ok(())
}

/// Refuse a raw command before canonical_ids resolves cross-workspace aliases for a write key.
pub fn precheck_raw(value: &Value) -> Result<()> {
    if current().is_none() {
        return Ok(());
    }
    ensure!(
        value["op"] == "save_object",
        "a workspace write key may only update key_write fields with save_object"
    );
    ensure!(
        value["data"].get("workspace").is_none(),
        "a workspace write key cannot change workspace"
    );
    Ok(())
}

/// True when `child` differs from the stored `data.workspace` of `(kind, id)`, or outside a write key.
pub fn check_child_needed(w: &Workspace, kind: &str, id: &str, child: &str) -> bool {
    if current().is_none() {
        return true;
    }
    let addr = address(kind, id);
    w.objects
        .get(resolve_address(w, &addr))
        .and_then(|o| o.current().data["workspace"].as_str())
        != Some(child)
}

/// Refuse a replayed write-key receipt when a sent field lost `key_write` in the latest schema.
pub async fn check_replayed(
    tx: &impl tokio_postgres::GenericClient,
    workspace: &str,
    operations: &[crate::store::BatchOperation],
) -> Result<()> {
    if current().is_none() {
        return Ok(());
    }
    let row = tx
        .query_opt(
            "SELECT value::text FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' ORDER BY revision DESC LIMIT 1",
            &[&workspace],
        )
        .await?;
    let types = match row.and_then(|r| r.get::<_, Option<String>>(0)) {
        Some(text) => serde_json::from_str::<crate::objects::SchemaRevision>(&text)?.types,
        None => crate::objects::legacy_defaults(),
    };
    for op in operations {
        let cmd = &op.command;
        ensure!(
            cmd["op"] == "save_object",
            "a workspace write key may only update key_write fields with save_object"
        );
        let kind = cmd["kind"].as_str().unwrap_or_default();
        let fields = types.get(kind).map(|t| &t.fields);
        if let Some(sent) = cmd["data"].as_object() {
            for field in sent.keys() {
                let ok = fields
                    .and_then(|f| f.get(field))
                    .is_some_and(|r| r.key_write && r.source() == "own");
                ensure!(
                    ok,
                    "field {field} of {kind} does not allow key_write in the tree schema"
                );
            }
        }
    }
    Ok(())
}

/// A command that only sets the hidden flag of the sender: update_workspace with hidden.
pub fn hides_only(command: &Value) -> bool {
    command["op"] == "update_workspace"
        && command.get("hidden").is_some_and(Value::is_boolean)
        && command.as_object().is_some_and(|m| {
            m.iter()
                .all(|(k, v)| k == "op" || k == "hidden" || v.is_null())
        })
}
