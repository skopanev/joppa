//! Part of `store_write`: `receipts_version` and the items after it, moved from store_write.rs unchanged.
use super::*;

pub(crate) const LEGACY_RECEIPTS_VERSION: &str = "1";

pub(crate) const BATCH_POSITIONS: &str = "WITH batch_positions AS MATERIALIZED (SELECT DISTINCT (operation->>'position')::bigint AS position FROM joppa.command_receipts r CROSS JOIN LATERAL jsonb_array_elements(r.result->'results') operation WHERE r.workspace=$1 AND r.input->>'batch'='true')";

pub(crate) async fn legacy_receipts_ready(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<bool> {
    Ok(client.query_opt("SELECT first_view->>'legacy_receipts_version' FROM joppa.board_cache WHERE workspace=$1", &[&workspace]).await?
        .is_some_and(|r| r.get::<_,Option<String>>(0).as_deref() == Some(LEGACY_RECEIPTS_VERSION)))
}

pub(crate) async fn migrate_legacy_receipts(client: &impl GenericClient) -> Result<()> {
    for row in client.query("SELECT workspace FROM joppa.board_cache WHERE COALESCE(first_view->>'legacy_receipts_version','')<>$1 ORDER BY workspace", &[&LEGACY_RECEIPTS_VERSION]).await? {
        let workspace: String = row.get(0);
        client.execute("SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))", &[&workspace]).await?;
        migrate_legacy_receipts_workspace(client, &workspace).await?;
    }
    Ok(())
}

pub(crate) async fn migrate_legacy_receipts_workspace(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<()> {
    if legacy_receipts_ready(client, workspace).await? {
        return Ok(());
    }
    client.execute(&format!("{BATCH_POSITIONS} INSERT INTO joppa.command_receipts(workspace,request_id,actor,input,result,recorded_at) SELECT e.workspace,e.request_id,e.actor,jsonb_build_object('legacy_command',e.command),jsonb_build_object('position',e.position),e.recorded_at FROM joppa.events e WHERE e.workspace=$1 AND NOT EXISTS(SELECT 1 FROM batch_positions b WHERE b.position=e.position) AND NOT EXISTS(SELECT 1 FROM joppa.command_receipts r WHERE r.workspace=e.workspace AND r.request_id=e.request_id) ON CONFLICT(workspace,request_id) DO NOTHING"), &[&workspace]).await?;
    let missing: bool = client.query_one(&format!("{BATCH_POSITIONS} SELECT EXISTS(SELECT 1 FROM joppa.events e WHERE e.workspace=$1 AND NOT EXISTS(SELECT 1 FROM batch_positions b WHERE b.position=e.position) AND NOT EXISTS(SELECT 1 FROM joppa.command_receipts r WHERE r.workspace=e.workspace AND r.request_id=e.request_id))"), &[&workspace]).await?.get(0);
    ensure!(!missing, "legacy request receipts are incomplete");
    client.execute("UPDATE joppa.board_cache SET first_view=jsonb_set(first_view,'{legacy_receipts_version}',to_jsonb($2::text)) WHERE workspace=$1", &[&workspace,&LEGACY_RECEIPTS_VERSION]).await?;
    Ok(())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BatchOperation {
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    pub command: Value,
}

pub async fn execute(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    request_id: &str,
    command: &Command,
) -> Result<Value> {
    execute_input(
        pool,
        workspace,
        actor,
        request_id,
        &serde_json::to_value(command)?,
    )
    .await
}

/// Enroll an agent or a runner through the internal enroll_service event. Only server code
/// calls it; HTTP and MCP writes refuse the operation.
pub async fn enroll_service(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    id: &str,
    role: &str,
) -> Result<Value> {
    let command = Command::EnrollService {
        id: id.into(),
        role: role.into(),
    };
    let request = format!("service-{}", uuid::Uuid::new_v4());
    execute(pool, workspace, actor, &request, &command).await
}

pub async fn execute_input(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    request_id: &str,
    command: &Value,
) -> Result<Value> {
    write(
        pool,
        workspace,
        actor,
        request_id,
        &[BatchOperation {
            reference: None,
            command: command.clone(),
        }],
        false,
        true,
    )
    .await
}

/// Public HTTP and MCP credentials may write inside existing workspaces only.
/// Provisioning a new workspace is reserved for the server-side CLI.
pub async fn execute_remote_input(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    request_id: &str,
    command: &Value,
) -> Result<Value> {
    write(
        pool,
        workspace,
        actor,
        request_id,
        &[BatchOperation {
            reference: None,
            command: command.clone(),
        }],
        false,
        false,
    )
    .await
}

/// A member adds or removes one blocking dependency of `source` on `target`. The ordinary
/// save writes the current document with only this link changed. An added link of a Task
/// pins the current revision of its target; the link of another node pins nothing.
pub async fn save_dependency(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    request_id: &str,
    source: &str,
    target: &str,
    add: bool,
) -> Result<Value> {
    let scope = super::ObjectScope {
        address: Some(source),
        ..Default::default()
    };
    let object = super::objects::read_stored_objects(pool, workspace, actor, &scope).await?;
    let document = &object["document"];
    let mut links: Vec<crate::objects::ObjectLink> =
        serde_json::from_value(document["links"].clone())?;
    let dependency =
        |l: &crate::objects::ObjectLink| l.relation == "depends_on" && l.target == target;
    let kind = object["kind"].as_str().context("object has no kind")?;
    let schema = super::tree_schema(pool, workspace, actor, None).await?;
    let before = links.len();
    if add {
        let revision = if schema["schema"]["types"][kind]["role"] == "task" {
            let scope = super::ObjectScope {
                address: Some(target),
                ..Default::default()
            };
            let target = super::read_objects(pool, workspace, actor, &scope).await?;
            Some(u32::try_from(
                target["current_revision"]
                    .as_u64()
                    .context("target has no revision")?,
            )?)
        } else {
            None
        };
        links.retain(|l| !dependency(l));
        links.push(crate::objects::ObjectLink {
            target: target.into(),
            relation: "depends_on".into(),
            revision,
            blocking: true,
        });
    } else {
        links.retain(|l| !(dependency(l) && l.blocking));
        ensure!(links.len() < before, "unknown dependency");
    }
    let reason = if add {
        format!("Depend on {target}")
    } else {
        format!("Remove the dependency on {target}")
    };
    let mut command = json!({"op":"save_object","kind":kind,"id":object["id"],"revision":object["current_revision"],
        "parent":document["parent"],"data":document["data"],"links":links,"reason":reason});
    // Only a link changes, so the title and the description still answer the type question.
    if let Some(question) = schema["schema"]["types"][kind]["question"].as_str() {
        command["question_checked"] = json!(question);
    }
    execute_remote_input(pool, workspace, actor, request_id, &command).await
}

pub async fn execute_batch(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    request_id: &str,
    operations: &[BatchOperation],
) -> Result<Value> {
    write(pool, workspace, actor, request_id, operations, true, true).await
}

pub async fn execute_remote_batch(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    request_id: &str,
    operations: &[BatchOperation],
) -> Result<Value> {
    write(pool, workspace, actor, request_id, operations, true, false).await
}
