use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;
use uuid::Uuid;

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
        "domain" | "capability" | "consumer" | "project" | "intake" | "comment" | "task"
        | "issue" | "link_source" => Some("id"),
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
fn resolve_claim_next(value: &mut Value, state: &Workspace, actor: &str) -> Result<()> {
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
    if let Some(project) = project {
        ensure!(state.projects.contains_key(project), "unknown project");
    }
    if let Some(purpose) = purpose {
        ensure!(
            ["implementation", "manual", "decision"].contains(&purpose),
            "unknown task purpose"
        );
    }
    let priorities = object
        .get("priorities")
        .map(|v| v.as_array().context("priorities must be an array"))
        .transpose()?;
    let mut priorities_parsed = Vec::new();
    if let Some(priorities) = priorities {
        ensure!(priorities.len() <= 100, "too many priorities");
        for value in priorities {
            let selector = value.as_str().context("priority must be kind:id")?;
            let (kind, id) = selector
                .split_once(':')
                .context("priority must be kind:id")?;
            ensure!(!id.is_empty(), "priority ID is empty");
            let exists = match kind {
                "domain" => state.domains.contains_key(id),
                "capability" => state.capabilities.contains_key(id),
                "requirement" => state.requirements.contains_key(id),
                "ac" => state.requirements.values().any(|q| {
                    q.revisions
                        .last()
                        .is_some_and(|r| r.acs.iter().any(|ac| ac.id == id))
                }),
                "task" => state
                    .requirements
                    .values()
                    .any(|q| q.revisions.last().is_some_and(|r| r.tasks.contains_key(id))),
                "project" => state.projects.contains_key(id),
                "consumer" => state.consumers.contains_key(id),
                _ => anyhow::bail!("unknown priority kind: {kind}"),
            };
            ensure!(exists, "unknown priority target: {selector}");
            priorities_parsed.push((kind, id));
        }
    }
    let chosen = state
        .requirements
        .iter()
        .filter_map(|(req, q)| q.revisions.last().map(|r| (req, q, r)))
        .flat_map(|(req, q, r)| r.tasks.iter().map(move |(id, task)| (req, q, r, id, task)))
        .filter(|(_, _, _, _, task)| {
            project.is_none_or(|p| task.projects.iter().any(|id| id == p))
                && purpose.is_none_or(|p| task.purpose == p)
        })
        .filter(|(_, _, r, _, task)| checks::claimable(state, actor, r, task).is_ok())
        .min_by_key(|(req, q, r, id, task)| {
            let rank = priorities_parsed
                .iter()
                .position(|(kind, target)| match *kind {
                    "domain" => state
                        .capabilities
                        .get(&q.capability)
                        .is_some_and(|c| c.domain == *target),
                    "capability" => q.capability == *target,
                    "requirement" => *req == target,
                    "ac" => task.acs.iter().any(|ac| ac == target),
                    "task" => *id == target,
                    "project" => task.projects.iter().any(|project| project == target),
                    "consumer" => r.consumers.iter().any(|consumer| consumer == target),
                    _ => false,
                })
                .unwrap_or(priorities_parsed.len());
            (rank, task.created_at, (*req).clone(), (*id).clone())
        })
        .map(|(req, _, r, id, _)| (req.clone(), r.number, id.clone()))
        .context("no available Task matches this claim")?;
    *value = json!({"op":"claim_task","req":chosen.0,"revision":chosen.1,"id":chosen.2});
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
        ensure!(
            operations.iter().all(|operation| operation.command["op"]
                .as_str()
                .is_some_and(|op| op != "workspace")),
            "workspace creation is available only through the server CLI"
        );
    }
    let input = json!({"batch":batch, "operations":operations});
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let workspace = resolve_workspace(&*tx, workspace).await?;
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&workspace],
    )
    .await?;
    // A new workspace has no access-rule entry yet. Only the trusted CLI path
    // may create it; public HTTP/MCP writes always go through access::check.
    let cli_create = allow_workspace_create && operations[0].command["op"] == "workspace";
    if !cli_create {
        access::check(&*tx, &workspace, actor, false).await?;
    }
    ensure!(actor != "joppa-access", "internal enrollment operation");
    if let Some(row) = tx.query_opt("SELECT actor,input::text,result::text FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2", &[&workspace,&request_id]).await? {
        ensure!(row.get::<_, String>(0) == actor && serde_json::from_str::<Value>(row.get::<_, &str>(1))? == input, "request_id reused with different content/actor");
        let mut result: Value = serde_json::from_str(row.get::<_, &str>(2))?;
        result["replayed"] = json!(true);
        return Ok(result);
    }
    // Old single-operation requests retain their idempotency contract after upgrade.
    if let Some(row) = tx.query_opt("SELECT actor,command::text,position FROM joppa.events WHERE workspace=$1 AND request_id=$2", &[&workspace,&request_id]).await? {
        ensure!(!batch, "request_id already used by an operation");
        let command: Command = serde_json::from_value(operations[0].command.clone())?;
        let previous: Command = serde_json::from_str(row.get::<_, &str>(1))?;
        ensure!(row.get::<_, String>(0) == actor && serde_json::to_value(&previous)? == serde_json::to_value(&command)?, "request_id reused with different content/actor");
        return Ok(json!({"position":row.get::<_, i64>(2), "command":command, "replayed":true}));
    }
    let (mut state, mut events) = load(&*tx, &workspace).await?;
    let mut recorded_state = state.clone();
    let base_position = events.len();
    let mut refs = BTreeMap::new();
    let mut results = vec![];
    let batch_id = Uuid::new_v4();
    for (index, operation) in operations.iter().enumerate() {
        let apply = async {
            if let Some(name) = &operation.reference {
                commands::identifier(name)?;
                ensure!(!refs.contains_key(name), "duplicate batch reference");
            }
            let mut value = resolve(&operation.command, &refs)?;
            generate(&mut value)?;
            resolve_claim_next(&mut value, &state, actor)?;
            let command: Command = serde_json::from_value(value)?;
            if let Command::Revise { consumers, .. } = &command {
                ensure!(!consumers.is_empty(), "REQ requires at least one consumer");
            }
            if let Command::Task { acs, contract, .. } = &command {
                ensure!(acs.len() == 1, "task must belong to exactly one AC");
                commands::nonempty(contract)?;
            }
            ensure!(!matches!(command, Command::EnrollMember { .. }), "internal enrollment operation");
            ensure!(
                allow_workspace_create || !matches!(command, Command::Workspace { .. }),
                "workspace creation is available only through the server CLI"
            );
            let position = (base_position + index + 1) as u64;
            // Validate against the in-memory state, then apply the recorded DB time
            // to the projection so notes and status dates need no second replay.
            commands::apply(&mut state, actor, &command, position, "")?;
            if position == 1 {
                tx.execute("INSERT INTO joppa.workspaces(id) VALUES($1)", &[&workspace]).await?;
                reserve_name(&*tx, &workspace, &workspace).await?;
            }
            if let Command::RenameWorkspace { workspace_id: Some(name), .. } = &command {
                reserve_name(&*tx, name, &workspace).await?;
            }
            let event_request = if batch { format!("batch-{batch_id}-{index}") } else { request_id.to_owned() };
            let payload = serde_json::to_string(&command)?;
            let recorded_at:String=tx.query_one("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,$3,$4,$5::text::jsonb) RETURNING recorded_at::text", &[&workspace,&(position as i64),&actor,&event_request,&payload]).await?.get(0);
            commands::apply(&mut recorded_state, actor, &command, position, &recorded_at)?;
            events.push(Event{position,actor:actor.to_owned(),recorded_at,command:command.clone()});
            if let Some(name) = &operation.reference { refs.insert(name.clone(), serde_json::to_value(&command)?); }
            Ok::<_, anyhow::Error>(json!({"position":position,"command":command,"system_ids":item_ids(&*tx,&workspace,&state,&command).await?}))
        }.await.with_context(|| format!("operation {}", index + 1))?;
        results.push(apply);
    }
    super::search::project(&*tx, &workspace, &recorded_state, events.len() as u64).await?;
    super::board::project(&*tx, &workspace, &recorded_state, &events).await?;
    let mut result = if batch {
        json!({"results":results})
    } else {
        results.remove(0)
    };
    result["replayed"] = json!(false);
    result["summary"] = summaries(&recorded_state);
    tx.execute("INSERT INTO joppa.command_receipts(workspace,request_id,actor,input,result) VALUES($1,$2,$3,$4::text::jsonb,$5::text::jsonb)", &[&workspace,&request_id,&actor,&input.to_string(),&result.to_string()]).await?;
    tx.commit().await?;
    Ok(result)
}
