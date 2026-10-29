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

/// Append the persistent claim migration of a workspace that has an owner and no such event.
pub(super) async fn migrate_claims(
    tx: &impl GenericClient,
    workspace: &str,
    state: &mut Workspace,
    events: &mut Vec<Event>,
    base_position: u64,
) -> Result<()> {
    if !state.admin.is_empty() && !state.persistent_claims {
        let command = Command::MigratePersistentClaims {};
        let position = base_position + 1;
        let migration_actor = state.admin.clone();
        let payload = serde_json::to_string(&command)?;
        let recorded_at: String = tx.query_one("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,$3,$4,$5::text::jsonb) RETURNING recorded_at::text", &[&workspace,&(position as i64),&migration_actor,&Uuid::new_v4().to_string(),&payload]).await?.get(0);
        commands::apply_replay(state, &migration_actor, &command, position, &recorded_at)?;
        events.push(Event {
            position,
            actor: migration_actor.clone(),
            actor_type: crate::inbox::actor_type(state, &migration_actor).into(),
            recorded_at,
            command,
        });
    }
    Ok(())
}

/// A claim, a completion and an acceptance read the workspaces of blocking remote links.
pub(super) fn locks_remote(operations: &[BatchOperation]) -> bool {
    operations.iter().any(|o| {
        matches!(
            o.command["op"].as_str(),
            Some(
                "claim_next_task"
                    | "claim_task"
                    | "claim_planning"
                    | "complete_task"
                    | "accept_commit"
            )
        )
    })
}
