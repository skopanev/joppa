//! Part of `tasks`: `save` and the items after it, moved from tasks.rs unchanged.
use super::*;

pub fn save(
    w: &mut Workspace,
    actor: &str,
    key: &str,
    data: &Value,
    parent: Option<&str>,
    position: u64,
    unchanged: impl Fn(&Value) -> bool,
) -> Result<Value> {
    let mut data = data.clone();
    let old = w.objects.get(key);
    let owner = data
        .get("owner")
        .and_then(Value::as_str)
        .or_else(|| old.and_then(|o| o.current().data["owner"].as_str()))
        .or_else(|| {
            parent
                .and_then(|p| w.objects.get(p))
                .and_then(|o| o.current().data["owner"].as_str())
        })
        .context("Task needs an owner")?
        .to_owned();
    commands::human(w, &owner)?;
    if let Some(old) = old {
        ensure!(
            old.current().data["owner"] == owner
                || old.current().data["owner"] == actor
                || actor == w.admin,
            "only current owner can transfer responsibility"
        );
        ensure!(
            old.current().parent.as_deref() == parent
                || old.current().data["owner"] == actor
                || actor == w.admin,
            "Task owner required to change its parent"
        );
    }
    let string = |field: &str| {
        data[field]
            .as_str()
            .with_context(|| format!("missing {field}"))
    };
    commands::nonempty(string("title")?)?;
    commands::nonempty(string("contract")?)?;
    let purpose = string("purpose")?.to_owned();
    ensure!(!purpose.trim().is_empty(), "task purpose must be nonempty");
    let kind = key.split_once(':').context("Task address needs a type")?.0;
    let rules = objects::schema(w)
        .types
        .get(kind)
        .cloned()
        .context("unknown Task type")?;
    let mut projects: Vec<String> = if w.schema_migrated {
        objects::reference_targets(w, kind, &data)?
            .into_iter()
            .filter(|(_, _, subject)| *subject)
            .map(|(_, target, _)| target.split_once(':').unwrap().1.to_owned())
            .collect()
    } else {
        let projects: Vec<String> =
            serde_json::from_value(data.get("projects").cloned().unwrap_or_else(|| json!([])))?;
        ensure!(
            projects.iter().all(|p| w.projects.contains_key(p)),
            "unknown Task project"
        );
        projects
    };
    if w.schema_migrated
        && let Some(parent) = parent
    {
        projects.extend(subjects(w, parent)?);
        projects.sort();
        projects.dedup();
    }
    check_projects(w, parent, &projects)?;
    for field in data.as_object().context("data must be an object")?.keys() {
        ensure!(
            ["title", "owner", "contract", "purpose"].contains(&field.as_str())
                || w.schema_migrated
                    && !objects::process_field(field)
                    && (w.documents_migrated || field != "projects")
                || w.schema_migrated
                    && rules.references.contains_key(field)
                    && !objects::process_field(field)
                || field == "projects"
                    && (!w.schema_migrated || data[field].as_array().is_some_and(Vec::is_empty)),
            "unknown Task document field {field}; use process commands for evidence"
        );
    }
    let title = string("title")?.to_owned();
    let contract = string("contract")?.to_owned();
    data["owner"] = json!(owner);
    if !w.schema_migrated {
        data["projects"] = json!(projects);
    }
    let mut t = task(w, key).cloned().unwrap_or_else(|| Task {
        claim: None,
        title: String::new(),
        acs: vec![],
        projects: vec![],
        purpose: purpose.clone(),
        assignee: None,
        done: None,
        commit: None,
        contract: String::new(),
        created_at: position,
        created_by: actor.into(),
        review_required: false,
        origin_revision: 1,
        retired: None,
        blocked: None,
    });
    if let Some(old) = old {
        ensure!(
            unchanged(&data) || (t.done.is_none() && t.retired.is_none()),
            "completed or retired Task cannot be amended"
        );
        ensure!(
            old.current().data["owner"] == actor
                || holder(w, &t) == Some(actor)
                || holder(w, &t).is_none() && t.created_by == actor
                || actor == w.admin
                || old.current().parent.as_deref().is_some_and(|p| {
                    parent.map(|q| objects::resolve_address(w, q))
                        == Some(objects::resolve_address(w, p))
                        && old.current().data["owner"] == owner
                        && w.planning_claims
                            .get(objects::resolve_address(w, p))
                            .is_some_and(|c| c.actor == actor)
                }),
            "Task owner, assignee or unclaimed creator required"
        );
        ensure!(t.purpose == purpose, "task purpose cannot change");
    }
    t.title = title;
    t.contract = contract;
    t.projects = projects;
    t.acs.clear();
    t.origin_revision = old.map_or(1, |o| o.current().revision + 1);
    if old.is_none() {
        let pin = parent.and_then(|p| {
            w.objects.get(p).map(|o| {
                (
                    p.to_owned(),
                    w.task_process
                        .get(p)
                        .map_or(objects::contract_revision(o), |t| t.origin_revision),
                )
            })
        });
        w.task_context.insert(key.into(), pin);
    }
    w.task_process.insert(key.into(), t);
    Ok(data)
}

pub fn process(w: &Workspace, key: &str) -> Option<Value> {
    let t = w.task_process.get(key)?;
    let o = w.objects.get(key)?;
    let mut p = serde_json::to_value(t).ok()?;
    p["revision"] = json!(o.current().revision);
    p["review_required"] = json!(review_required(w, key, t));
    p["status"] = json!(status(t));
    p["blocked"] = json!(t.blocked);
    p["assessment"] = json!(parent_assessment(w, key));
    p["readiness_blocker"] = json!(readiness_blocker(w, key));
    Some(p)
}
