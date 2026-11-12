//! Part of `store_write`: `register`, `item_ids` and the items after them.
use super::*;

/// Register the public item ID of a saved object. An MCP write stops here: it reads no
/// `system_ids` (tac:661jb0ex).
pub(crate) async fn register(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    command: &Command,
) -> Result<()> {
    if let Command::SaveObject(crate::commands::SaveObjectOp { kind, id, .. }) = command {
        let (original_kind, original_id) = crate::objects::original_identity(state, kind, id);
        client
            .execute(
                "SELECT joppa.register_item($1,$2,$3,$4)",
                &[
                    &workspace,
                    &original_kind,
                    &original_id,
                    &state.reference_prefix.as_deref().unwrap_or(workspace),
                ],
            )
            .await?;
    }
    Ok(())
}

pub(crate) async fn item_ids(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    command: &Command,
) -> Result<Value> {
    register(client, workspace, state, command).await?;
    let mut keys = BTreeMap::<&str, Vec<String>>::new();
    match command {
        Command::SaveObject(crate::commands::SaveObjectOp { kind, id, .. }) => {
            let (original_kind, original_id) = crate::objects::original_identity(state, kind, id);
            if original_kind != kind || original_id != id {
                let row = client.query_one("SELECT id FROM joppa.item_ids WHERE workspace=$1 AND namespace=$2 AND kind=$3 AND local_id=$4", &[&workspace,&state.reference_prefix.as_deref().unwrap_or(workspace),&original_kind,&original_id]).await?;
                return Ok(json!({kind: {id: row.get::<_,String>(0)}}));
            }
            keys.entry(kind).or_default().push(id.clone());
        }
        Command::Domain(crate::commands::DomainOp { id, .. }) => {
            keys.insert("domain", vec![id.clone()]);
        }
        Command::Capability(crate::commands::CapabilityOp { id, .. }) => {
            keys.insert("capability", vec![id.clone()]);
        }
        Command::Consumer(crate::commands::ConsumerOp { id, .. }) => {
            keys.insert("consumer", vec![id.clone()]);
        }
        Command::Revise(crate::commands::ReviseOp { req, acs, .. }) => {
            keys.insert("requirement", vec![req.clone()]);
            keys.insert("ac", acs.iter().map(|ac| ac.id.clone()).collect());
        }
        _ => {}
    }
    let mut ids = json!({});
    for (kind, local) in keys {
        ids[kind] = json!({});
        for row in client.query("SELECT local_id,id FROM joppa.item_ids WHERE workspace=$1 AND namespace=$2 AND kind=$3 AND local_id=ANY($4)", &[&workspace, &state.reference_prefix.as_deref().unwrap_or(workspace), &kind, &local]).await? {
            let local: String = row.get(0);
            ids[kind][local] = json!(row.get::<_, String>(1));
        }
    }
    Ok(ids)
}

/// Public writes accept only catalog operations, before alias resolution and any write.
/// Remote workspace creation keeps its own refusal.
pub(crate) fn admit(operations: &[BatchOperation], batch: bool) -> Result<()> {
    fn op(operation: &BatchOperation) -> Option<&str> {
        operation.command["op"].as_str()
    }
    ensure!(
        operations.iter().all(|o| op(o) != Some("workspace")),
        "workspace creation is available only through the server CLI"
    );
    for (index, operation) in operations.iter().enumerate() {
        let admitted = match op(operation) {
            Some(name) if crate::removed_ops::NAMES.contains(&name) => {
                Err(anyhow::anyhow!(crate::removed_ops::refusal(name)))
            }
            Some(name) if commands::operation(name).is_some() => Ok(()),
            _ => Err(anyhow::anyhow!("unknown operation")),
        };
        if batch {
            admitted.with_context(|| format!("operation {}", index + 1))?;
        } else {
            admitted?;
        }
    }
    Ok(())
}

/// Field validation of one command after generated fields, before state checks.
pub(crate) fn fields(value: &Value) -> Result<()> {
    serde_json::from_value::<Command>(value.clone())?;
    Ok(())
}

/// The field validation of the write path for one public command.
pub fn check_fields(command: &Value) -> Result<()> {
    let mut value = command.clone();
    generate(&mut value)?;
    fields(&value)
}
