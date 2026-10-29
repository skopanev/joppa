//! Part of `store_write`: `generate` and the items after it, moved from store_write.rs unchanged.
use super::*;

pub(crate) fn generate(command: &mut Value) -> Result<()> {
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
pub(crate) fn resolve_task(value: &mut Value, state: &Workspace) -> Result<()> {
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

pub(crate) fn resolve_claim_next(
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
pub(crate) fn resolve(value: &Value, refs: &BTreeMap<String, Value>) -> Result<Value> {
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
