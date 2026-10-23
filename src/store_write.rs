use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;
use uuid::Uuid;

const LEGACY_RECEIPTS_VERSION: &str = "1";
const BATCH_POSITIONS: &str = "WITH batch_positions AS MATERIALIZED (SELECT DISTINCT (operation->>'position')::bigint AS position FROM joppa.command_receipts r CROSS JOIN LATERAL jsonb_array_elements(r.result->'results') operation WHERE r.workspace=$1 AND r.input->>'batch'='true')";

async fn legacy_receipts_ready(client: &impl GenericClient, workspace: &str) -> Result<bool> {
    Ok(client.query_opt("SELECT first_view->>'legacy_receipts_version' FROM joppa.board_cache WHERE workspace=$1", &[&workspace]).await?
        .is_some_and(|r| r.get::<_,Option<String>>(0).as_deref() == Some(LEGACY_RECEIPTS_VERSION)))
}

pub(super) async fn migrate_legacy_receipts(client: &impl GenericClient) -> Result<()> {
    for row in client.query("SELECT workspace FROM joppa.board_cache WHERE COALESCE(first_view->>'legacy_receipts_version','')<>$1 ORDER BY workspace", &[&LEGACY_RECEIPTS_VERSION]).await? {
        let workspace: String = row.get(0);
        client.execute("SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))", &[&workspace]).await?;
        migrate_legacy_receipts_workspace(client, &workspace).await?;
    }
    Ok(())
}

pub(super) async fn migrate_legacy_receipts_workspace(
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
    let object = super::read_objects(pool, workspace, actor, &scope).await?;
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

fn generate(command: &mut Value) -> Result<()> {
    let op = command["op"].as_str().unwrap_or("").to_owned();
    let c = command
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("command must be an object"))?;
    let key = match op.as_str() {
        "save_object" | "domain" | "capability" | "consumer" | "project" | "intake" | "comment"
        | "task" | "issue" | "link_source" => Some("id"),
        "plan" => Some("run"),
        "revise" if c.get("expected_revision").and_then(Value::as_u64) == Some(0) => Some("req"),
        _ => None,
    };
    if let Some(key) = key {
        c.entry(key)
            .or_insert_with(|| json!(Uuid::new_v4().to_string()));
    }
    if op == "revise"
        && let Some(acs) = c.get_mut("acs").and_then(Value::as_array_mut)
    {
        for ac in acs {
            if let Some(ac) = ac.as_object_mut() {
                ac.entry("id")
                    .or_insert_with(|| json!(Uuid::new_v4().to_string()));
            }
        }
    }
    if op == "plan" {
        c.insert("enforce_coverage".into(), json!(true));
    }
    if op == "revise" {
        // New writes get the new policy; historical Revise events have no flag.
        c.insert("coverage_policy".into(), json!(true));
        c.insert("carry_tasks".into(), json!(true));
    }
    Ok(())
}

/// Resolve an atomic "take next" request under the workspace write lock.
/// Only the concrete claim is journaled; the receipt preserves the original
/// request for idempotent retries.
fn resolve_task(value: &mut Value, state: &Workspace) -> Result<()> {
    let op = value["op"].as_str().unwrap_or("").to_owned();
    let req = value["req"].as_str().unwrap_or("").to_owned();
    if req.is_empty()
        || ![
            "task",
            "claim_task",
            "amend_task",
            "release_task",
            "complete_task",
            "review_task",
            "retire_task",
        ]
        .contains(&op.as_str())
    {
        return Ok(());
    }
    let r = state
        .requirements
        .get(&req)
        .and_then(|q| q.revisions.last())
        .context("unknown REQ")?;
    ensure!(
        value["revision"].as_u64() == Some(r.number as u64),
        "stale REQ revision"
    );
    let id = value["id"].as_str().context("Task needs an id")?;
    let key = format!("task:{req}/{id}");
    if op == "task" {
        let acs: Vec<String> = serde_json::from_value(value["acs"].clone())?;
        ensure!(
            acs.len() == 1,
            "legacy task needs one AC; use save_object for the configured tree"
        );
        ensure!(r.acs.iter().any(|a| a.id == acs[0]), "unknown AC");
        ensure!(
            value.get("assignee").is_none_or(Value::is_null),
            "create an open Task, then claim it"
        );
        let owner = state
            .objects
            .get(&crate::objects::criterion_address(&acs[0]))
            .context("unknown Task parent")?
            .current()
            .data["owner"]
            .clone();
        *value = json!({"op":"save_object","kind":"task","id":format!("{req}/{id}"),"revision":0,
            "parent":format!("ac:{}",acs[0]),"data":{"title":value["title"],"owner":owner,"contract":value["contract"],"projects":value["projects"],"purpose":value["purpose"]},"reason":"Create a Task"});
    } else if state.task_process.contains_key(&key) {
        if op == "review_task" {
            let ac = value["ac"]
                .as_str()
                .context("legacy review_task needs an AC")?;
            ensure!(
                state
                    .objects
                    .get(&key)
                    .context("unknown Task document")?
                    .current()
                    .parent
                    .as_deref()
                    == Some(&crate::objects::criterion_address(ac)),
                "change the Task parent with save_object"
            );
            value.as_object_mut().unwrap().remove("ac");
        }
        value.as_object_mut().unwrap().remove("req");
        value["id"] = json!(key);
        value["revision"] = json!(
            state
                .objects
                .get(value["id"].as_str().unwrap())
                .context("unknown Task document")?
                .current()
                .revision
        );
    }
    Ok(())
}

fn resolve_claim_next(
    value: &mut Value,
    state: &Workspace,
    actor: &str,
    waits: &BTreeMap<String, String>,
) -> Result<()> {
    if value["op"] != "claim_next_task" {
        return Ok(());
    }
    let object = value
        .as_object()
        .context("claim_next_task must be an object")?;
    ensure!(
        object
            .keys()
            .all(|key| ["op", "project", "purpose", "priorities"].contains(&key.as_str())),
        "unknown claim_next_task field"
    );
    let project = object
        .get("project")
        .map(|v| v.as_str().context("project must be a string"))
        .transpose()?;
    let purpose = object
        .get("purpose")
        .map(|v| v.as_str().context("purpose must be a string"))
        .transpose()?;
    let priorities: Option<Vec<String>> = object
        .get("priorities")
        .map(|v| {
            serde_json::from_value(v.clone())
                .context("priorities must be an array of kind:id strings")
        })
        .transpose()?;
    let chosen = super::tasks::select_next(
        state,
        actor,
        project,
        purpose,
        &priorities.unwrap_or_default(),
        waits,
    )?
    .context("no available Task matches this claim")?;
    *value = json!({"op":"claim_task","revision":chosen.1,"id":chosen.2});
    if !chosen.0.is_empty() {
        value["req"] = json!(chosen.0);
    }
    Ok(())
}

// Explicit references avoid interpreting titles or user-provided text as templates.
fn resolve(value: &Value, refs: &BTreeMap<String, Value>) -> Result<Value> {
    match value {
        Value::Object(map) if map.contains_key("$ref") => {
            ensure!(map.len() == 1, "$ref must be the only field");
            let path = map["$ref"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("$ref must be a string"))?;
            let (name, path) = path
                .split_once('/')
                .ok_or_else(|| anyhow::anyhow!("use $ref: label/field"))?;
            refs.get(name)
                .and_then(|v| v.pointer(&format!("/{path}")))
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("unknown or forward batch reference: {name}/{path}"))
        }
        Value::Object(map) => Ok(Value::Object(
            map.iter()
                .map(|(k, v)| Ok((k.clone(), resolve(v, refs)?)))
                .collect::<Result<_>>()?,
        )),
        Value::Array(values) => Ok(Value::Array(
            values
                .iter()
                .map(|v| resolve(v, refs))
                .collect::<Result<_>>()?,
        )),
        _ => Ok(value.clone()),
    }
}

async fn item_ids(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    command: &Command,
) -> Result<Value> {
    let mut keys = BTreeMap::<&str, Vec<String>>::new();
    match command {
        Command::SaveObject { kind, id, .. } => {
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
            if original_kind != kind || original_id != id {
                let row = client.query_one("SELECT id FROM joppa.item_ids WHERE workspace=$1 AND namespace=$2 AND kind=$3 AND local_id=$4", &[&workspace,&state.reference_prefix.as_deref().unwrap_or(workspace),&original_kind,&original_id]).await?;
                return Ok(json!({kind: {id: row.get::<_,String>(0)}}));
            }
            keys.entry(kind).or_default().push(id.clone());
        }
        Command::Domain { id, .. } => {
            keys.insert("domain", vec![id.clone()]);
        }
        Command::Capability { id, .. } => {
            keys.insert("capability", vec![id.clone()]);
        }
        Command::Consumer { id, .. } => {
            keys.insert("consumer", vec![id.clone()]);
        }
        Command::Revise { req, acs, .. } => {
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
/// Remote workspace creation and heartbeat_task keep their own refusals.
fn admit(operations: &[BatchOperation], batch: bool) -> Result<()> {
    fn op(operation: &BatchOperation) -> Option<&str> {
        operation.command["op"].as_str()
    }
    ensure!(
        operations.iter().all(|o| op(o) != Some("workspace")),
        "workspace creation is available only through the server CLI"
    );
    for (index, operation) in operations.iter().enumerate() {
        let admitted = match op(operation) {
            Some("heartbeat_task") => Err(anyhow::anyhow!("heartbeat_task is no longer supported")),
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
/// Historical events keep their serde defaults; current writes must send these fields.
/// claim_next_task has only optional filters; the write path checks them under the lock.
fn fields(value: &Value) -> Result<()> {
    for (op, field, refusal) in [
        ("accept_commit", "commit", "acceptance needs a commit"),
        ("task", "contract", "Task needs a technical contract"),
    ] {
        if value["op"] == op && value.get(field).is_none_or(Value::is_null) {
            anyhow::bail!("missing field `{field}`: {refusal}");
        }
    }
    if value["op"] != "claim_next_task" {
        serde_json::from_value::<Command>(value.clone())?;
    }
    Ok(())
}

/// The field validation of the write path for one public command.
pub fn check_fields(command: &Value) -> Result<()> {
    let mut value = command.clone();
    generate(&mut value)?;
    fields(&value)
}

async fn write(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    request_id: &str,
    operations: &[BatchOperation],
    batch: bool,
    allow_workspace_create: bool,
) -> Result<Value> {
    commands::identifier(workspace)?;
    commands::identifier(request_id)?;
    ensure!(
        !operations.is_empty() && operations.len() <= 100,
        "supply between 1 and 100 operations"
    );
    if !allow_workspace_create {
        admit(operations, batch)?;
    }
    let input = json!({"batch":batch, "operations":operations});
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let workspace = resolve_workspace(&*tx, workspace).await?;
    // A claim, a completion and an acceptance read the workspaces of blocking remote links,
    // so they lock them with their own workspace in sorted ID order. A record of results
    // there waits for them, or comes first.
    let claims = operations.iter().any(|o| {
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
    });
    let remote_locked = if claims {
        super::remote::blocking_closure(&*tx, &workspace).await?
    } else {
        Default::default()
    };
    super::remote::lock_sorted(&*tx, &workspace, &remote_locked).await?;
    // A new workspace has no access-rule entry yet. Only the trusted CLI path
    // may create it; public HTTP/MCP writes always go through access::check.
    let cli_create = allow_workspace_create && operations[0].command["op"] == "workspace";
    if !cli_create {
        access::check(&*tx, &workspace, actor, false).await?;
    }
    ensure!(actor != "joppa-access", "internal enrollment operation");
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
            return Ok(result);
        }
        ensure!(row.get::<_, String>(0) == actor && stored == input, "request_id reused with different content/actor");
        let mut result: Value = serde_json::from_str(row.get::<_, &str>(2))?;
        result["replayed"] = json!(true);
        return Ok(result);
    }
    // Old single-operation requests retain their idempotency contract after upgrade.
    if !legacy_receipts_ready(&*tx, &workspace).await?
        && let Some(row) = tx.query_opt("SELECT actor,command::text,position FROM joppa.events WHERE workspace=$1 AND request_id=$2", &[&workspace,&request_id]).await? {
        ensure!(!batch, "request_id already used by an operation");
        let command: Command = serde_json::from_value(operations[0].command.clone())?;
        let previous: Command = serde_json::from_str(row.get::<_, &str>(1))?;
        ensure!(row.get::<_, String>(0) == actor && serde_json::to_value(&previous)? == serde_json::to_value(&command)?, "request_id reused with different content/actor");
        return Ok(json!({"position":row.get::<_, i64>(2), "command":command, "replayed":true}));
    }
    let (mut state, base_position) = state_rows::current(&*tx, &workspace).await?;
    let mut previous = state.clone();
    let mut events = Vec::new();
    if !state.admin.is_empty() && !state.persistent_claims {
        let command = Command::MigratePersistentClaims {};
        let position = base_position + 1;
        let migration_actor = state.admin.clone();
        let payload = serde_json::to_string(&command)?;
        let recorded_at: String = tx.query_one("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,$3,$4,$5::text::jsonb) RETURNING recorded_at::text", &[&workspace,&(position as i64),&migration_actor,&Uuid::new_v4().to_string(),&payload]).await?.get(0);
        commands::apply_replay(
            &mut state,
            &migration_actor,
            &command,
            position,
            &recorded_at,
        )?;
        events.push(Event {
            position,
            actor: migration_actor.clone(),
            actor_type: crate::inbox::actor_type(&state, &migration_actor).into(),
            recorded_at,
            command,
        });
    }
    let mut refs = BTreeMap::new();
    let mut results = vec![];
    let batch_id = Uuid::new_v4();
    for (index, operation) in operations.iter().enumerate() {
        let apply = async {
            commands::reject_actor_field(&operation.command)?;
            if let Some(name) = &operation.reference {
                commands::identifier(name)?;
                ensure!(!refs.contains_key(name), "duplicate batch reference");
            }
            let mut value = resolve(&operation.command, &refs)?;
            generate(&mut value)?;
            fields(&value)?;
            // The journal stores the immutable ID of another workspace, never a name.
            super::remote::canonical_ids(&*tx, &mut value).await?;
            if value["op"] == "claim_next_task" {
                state.claim_time_ms = Some(tx.query_one("SELECT floor(extract(epoch FROM clock_timestamp())*1000)::bigint", &[]).await?.get(0));
            }
            resolve_task(&mut value, &state)?;
            // Work waits for its unsatisfied ordering dependencies; the wait is never stored.
            let waits = if matches!(value["op"].as_str(), Some("claim_next_task" | "claim_task" | "claim_planning")) {
                super::waits::live(&*tx, &workspace, actor, &state, &remote_locked).await?
            } else {
                BTreeMap::new()
            };
            resolve_claim_next(&mut value, &state, actor, &waits)?;
            state_rows::hydrate_command(&*tx, &workspace, &mut state, &mut previous, &value).await?;
            state_rows::hydrate_revision(&*tx, &workspace, &mut state, &value).await?;
            let command: Command = serde_json::from_value(value)?;
            if let Command::SetRemoteLinks { target, links, .. } = &command {
                let source = crate::objects::resolve_address(&state, target).to_owned();
                super::remote::check_links(&*tx, &workspace, actor, &source, links).await?;
            }
            if let Command::SaveObject { data, .. } = &command
                && let Some(child) = data.get("workspace")
            {
                let child = child.as_str().context("workspace is a workspace ID")?;
                super::remote::check_child(&*tx, &workspace, actor, child).await?;
            }
            if let Command::SaveObject { kind, question_checked, .. } = &command {
                let schema = crate::objects::schema(&state);
                if let Some(question) = schema.types.get(kind).and_then(|t| t.question.as_ref())
                    && question_checked.as_ref() != Some(question) {
                    return Err(crate::objects::RequiredField(format!("Check that the title and description answer: {question}. Rewrite them if they do not. Then resend with question_checked")).into());
                }
            }
            if let Command::Revise { req, consumers, capability, .. } = &command {
                ensure!(capability.is_some() || req.contains(':'), "legacy revise requires a Capability; use save_object for a custom tree");
                if state.schema_migrated {
                    let mut data = state.objects.get(&crate::objects::requirement_address(req)).map(|o| o.current().data.clone()).unwrap_or_else(|| json!({}));
                    data["consumers"] = json!(consumers);
                    let req_address = crate::objects::requirement_address(req);
                    let kind = req_address.split_once(':').unwrap().0;
                    crate::objects::reference_targets(&state, kind, &data)?;
                } else { ensure!(!consumers.is_empty(), "REQ requires at least one consumer"); }
            }
            if let Command::AcceptCommit { target, commit: Some(commit), .. } = &command {
                super::reports::check_acceptance(&*tx, &workspace, &state, target, commit).await?;
                // An acceptance needs every blocking remote target of the node, its ancestors
                // and the nodes below it ready now.
                if let Some(target) = super::waits::remote_gate(&*tx, actor, &state, target, true, &remote_locked).await? {
                    anyhow::bail!("{}", crate::dependencies::reason(&target));
                }
            }
            if let Command::CompleteTask { req, id, .. } = &command {
                let task = crate::objects::resolve_address(&state, &crate::tasks::address(req, id)).to_owned();
                if let Some(target) = super::waits::remote_gate(&*tx, actor, &state, &task, false, &remote_locked).await? {
                    anyhow::bail!("{}", crate::dependencies::reason(&target));
                }
            }
            if let Command::ClaimPlanning { ac, revision } = &command {
                let target = crate::planning::validate_claim(&state, ac, *revision)?;
                let status = super::reports::planning_criterion_status(&*tx, &workspace, &state, &target).await?;
                ensure!(status["status"] != "green", "criterion is already verified");
                if let Some(dependency) = waits.get(&target) {
                    anyhow::bail!("{}", crate::dependencies::reason(dependency));
                }
            }
            if let Command::Task { acs, contract, .. } = &command {
                ensure!(acs.len() == 1, "task must belong to exactly one AC");
                commands::nonempty(contract)?;
            }
            // Only server code enrolls an agent or a runner; HTTP and MCP writes refuse it.
            ensure!(
                allow_workspace_create || !matches!(command, Command::EnrollService { .. }),
                "internal operation"
            );
            ensure!(!matches!(command, Command::EnrollMember { .. } | Command::RecordReport { .. } | Command::MigrateNotes {} | Command::MigrateTasks {} | Command::MigrateSchema {} | Command::MigrateDocuments {} | Command::MigrateTaskClaims {} | Command::MigratePersistentClaims {}), "internal operation");
            ensure!(
                allow_workspace_create || !matches!(command, Command::Workspace { .. }),
                "workspace creation is available only through the server CLI"
            );
            let mut position = base_position + events.len() as u64;
            // A save that changes nothing keeps the current revision and adds no journal event.
            if !commands::unchanged_save(&state, actor, &command, position + 1, "")? {
                position += 1;
                if position == 1 {
                    tx.execute("INSERT INTO joppa.workspaces(id) VALUES($1)", &[&workspace]).await?;
                    super::start_replay_epochs(&*tx, &workspace).await?;
                    reserve_name(&*tx, &workspace, &workspace).await?;
                }
                if let Command::RenameWorkspace { workspace_id: Some(name), .. } = &command {
                    reserve_name(&*tx, name, &workspace).await?;
                }
                let event_request = if batch { format!("batch-{batch_id}-{index}") } else { request_id.to_owned() };
                let payload = serde_json::to_string(&command)?;
                let recorded_at:String=tx.query_one("WITH context AS MATERIALIZED (SELECT set_config('joppa.reference_prefix',$6,true)) INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1,$2,$3,$4,$5::text::jsonb FROM context RETURNING recorded_at::text", &[&workspace,&(position as i64),&actor,&event_request,&payload,&state.reference_prefix.as_deref().unwrap_or(&workspace)]).await?.get(0);
                commands::apply(&mut state, actor, &command, position, &recorded_at)?;
                // The Task pin, block and hold refusals come first; the refused write rolls back.
                if let Command::ClaimTask { req, id, .. } = &command
                    && let Some(dependency) = waits.get(crate::objects::resolve_address(&state, &crate::tasks::address(req, id)))
                {
                    anyhow::bail!("{}", crate::dependencies::reason(dependency));
                }
                crate::objects::validate_authoring_questions(&state, &command, position)?;
                crate::objects::validate_tree(&state)?;
                events.push(Event{position,actor:actor.to_owned(),actor_type:crate::inbox::actor_type(&state, actor).into(),recorded_at,command:command.clone()});
            }
            let mut resolved=serde_json::to_value(&command)?;
            let saved_address = match &command {
                Command::SaveObject { id, kind, .. } => Some(format!("{kind}:{id}")),
                Command::SetNoteStatus { target, .. } | Command::DecideNote { target, .. } => Some(crate::objects::resolve_address(&state, target).to_owned()),
                _ => None,
            };
            let saved=if let Some(address)=saved_address {
                let revision=state.objects[&address].current().revision;
                resolved["address"]=json!(address);resolved["saved_revision"]=json!(revision);
                let mut saved = json!({"address":address,"revision":revision});
                if let Command::SaveObject { kind, .. } = &command
                    && let Some(question) = crate::objects::schema(&state).types[kind].question.as_ref() {
                    saved["question"] = json!(question);
                }
                if let Some(hint) = crate::ac_tests::draft_hint(&state, &address) {
                    saved["tests_draft"] = json!(true);
                    saved["hint"] = json!(hint);
                }
                Some(saved)
            }else{None};
            if let Some(name) = &operation.reference { refs.insert(name.clone(), resolved); }
            Ok::<_, anyhow::Error>(json!({"position":position,"command":command,"object":saved,"system_ids":item_ids(&*tx,&workspace,&state,&command).await?}))
        }.await;
        let apply = if batch {
            apply.with_context(|| format!("operation {}", index + 1))
        } else {
            apply
        }?;
        results.push(apply);
    }
    ensure_notes(&*tx, &workspace, &mut state, &mut events, base_position).await?;
    let selected = state_rows::affected(&state, Some(&previous))?;
    super::search::project(
        &*tx,
        &workspace,
        &state,
        base_position + events.len() as u64,
        selected.as_ref(),
    )
    .await?;
    super::board::project_current(
        &*tx,
        &workspace,
        &state,
        &events,
        base_position + events.len() as u64,
        Some(&previous),
    )
    .await?;
    let mut result = if batch {
        json!({"results":results})
    } else {
        results.remove(0)
    };
    result["replayed"] = json!(false);
    result["summary"] = state_rows::changed_summaries(&state, selected.as_ref());
    tx.execute("INSERT INTO joppa.command_receipts(workspace,request_id,actor,input,result) VALUES($1,$2,$3,$4::text::jsonb,$5::text::jsonb)", &[&workspace,&request_id,&actor,&input.to_string(),&result.to_string()]).await?;
    if base_position == 0 {
        tx.execute("UPDATE joppa.board_cache SET first_view=jsonb_set(first_view,'{legacy_receipts_version}',to_jsonb($2::text)) WHERE workspace=$1", &[&workspace,&LEGACY_RECEIPTS_VERSION]).await?;
    }
    tx.commit().await?;
    Ok(result)
}
