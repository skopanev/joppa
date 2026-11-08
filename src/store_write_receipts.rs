//! The parts of a write before its operations: a retry of a stored request, the persistent
//! claim migration and the operations that lock the workspaces of remote links.
use super::*;

/// The stored reply of an earlier identical request, or None for a new request.
#[allow(clippy::too_many_arguments)]
pub(super) async fn replayed(
    tx: &impl GenericClient,
    workspace: &str,
    actor: &str,
    request_id: &str,
    operations: &[BatchOperation],
    batch: bool,
    input: &Value,
) -> Result<Option<Value>> {
    if let Some(row) = tx.query_opt("SELECT actor,input::text,result::text FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2", &[&workspace,&request_id]).await? {
    let stored: Value = serde_json::from_str(row.get::<_, &str>(1))?;
    if let Some(raw) = stored.get("legacy_command") {
        ensure!(!batch, "request_id already used by an operation");
        let command: Command = serde_json::from_value(operations[0].command.clone())?;
        let previous: Command = serde_json::from_value(raw.clone())?;
        ensure!(row.get::<_, String>(0) == actor && serde_json::to_value(&previous)? == serde_json::to_value(&command)?, "request_id reused with different content/actor");
        let mut result: Value = serde_json::from_str(row.get::<_, &str>(2))?;
        result["command"] = json!(command);
        result["replayed"] = json!(true);
        return Ok(Some(result));
    }
    ensure!(row.get::<_, String>(0) == actor && &stored == input, "request_id reused with different content/actor");
    let mut result: Value = serde_json::from_str(row.get::<_, &str>(2))?;
    result["replayed"] = json!(true);
    return Ok(Some(result));
}
    // Old single-operation requests retain their idempotency contract after upgrade.
    if !legacy_receipts_ready(tx, workspace).await?
    && let Some(row) = tx.query_opt("SELECT actor,command::text,position FROM joppa.events WHERE workspace=$1 AND request_id=$2", &[&workspace,&request_id]).await? {
    ensure!(!batch, "request_id already used by an operation");
    let command: Command = serde_json::from_value(operations[0].command.clone())?;
    let previous: Command = serde_json::from_str(row.get::<_, &str>(1))?;
    ensure!(row.get::<_, String>(0) == actor && serde_json::to_value(&previous)? == serde_json::to_value(&command)?, "request_id reused with different content/actor");
    return Ok(Some(json!({"position":row.get::<_, i64>(2), "command":command, "replayed":true})));
}
    Ok(None)
}
