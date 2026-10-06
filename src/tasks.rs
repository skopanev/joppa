use crate::{
    commands::{self, Command},
    model::{Task, Workspace},
    objects,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub fn address(req: &str, id: &str) -> String {
    if !req.is_empty() {
        format!("task:{req}/{id}")
    } else if id.contains(':') {
        id.into()
    } else {
        format!("task:{id}")
    }
}

pub fn task<'a>(w: &'a Workspace, key: &str) -> Option<&'a Task> {
    let key = objects::resolve_address(w, key);
    w.task_process.get(key).or_else(|| {
        let (req, id) = key.strip_prefix("task:")?.split_once('/')?;
        w.requirements.get(req)?.revisions.last()?.tasks.get(id)
    })
}

pub fn status(t: &Task) -> &'static str {
    if t.retired.is_some() {
        "retired"
    } else if t.done.is_some() {
        "done"
    } else if t.assignee.is_some() {
        "in_progress"
    } else {
        "open"
    }
}

pub fn display_tasks(w: &Workspace, req: &str) -> std::collections::BTreeMap<String, Task> {
    let mut result = std::collections::BTreeMap::new();
    for (key, t) in &w.task_process {
        if !objects::within(w, key, &format!("requirement:{req}")) {
            continue;
        }
        let mut t = t.clone();
        t.review_required = review_required(w, key, &t);
        t.acs = w.objects[key]
            .current()
            .parent
            .as_deref()
            .and_then(|p| p.strip_prefix("ac:"))
            .map(|id| vec![id.into()])
            .unwrap_or_default();
        let id = key.strip_prefix(&format!("task:{req}/")).unwrap_or(key);
        result.insert(id.into(), t);
    }
    result
}

fn context(w: &Workspace, key: &str) -> Option<(String, u32)> {
    let p = w.objects.get(key)?.current().parent.as_ref()?;
    Some((
        p.clone(),
        w.task_process
            .get(p)
            .map_or(w.objects.get(p)?.current().revision, |t| t.origin_revision),
    ))
}

pub fn review_required(w: &Workspace, key: &str, t: &Task) -> bool {
    t.review_required
        || w.task_context
            .get(key)
            .is_some_and(|old| old != &context(w, key))
}

fn parent_ready(w: &Workspace, actor: &str, key: &str, purpose: &str) -> Result<()> {
    let o = w.objects.get(key).context("unknown Task")?;
    let schema = objects::schema(w);
    let Some(gate) = schema
        .types
        .get(&o.kind)
        .context("Task type is absent from schema")?
        .parent_gate
        .as_deref()
    else {
        return Ok(());
    };
    let Some(parent) = o.current().parent.as_deref() else {
        return Ok(());
    };
    w.objects.get(parent).context("unknown Task parent")?;
    let process = objects::process(w, parent);
    let ready = match gate {
        "completed" => task(w, parent).is_some_and(|t| t.done.is_some() && t.retired.is_none()),
        "verified" => {
            process["status"]["verification"] == "pass" || process["summary"]["accepted"] == true
        }
        "ready" => {
            if let Some((_, r, ac)) = objects::criterion_context(w, parent) {
                if purpose == "decision"
                    && (!w.schema_migrated || schema.types[&o.kind].parent_decision)
                {
                    commands::human(w, actor)?;
                    ac.depends_on.iter().all(|id| commands::ready(r, id))
                } else {
                    commands::ready(r, &ac.id)
                }
            } else {
                objects::work_ready(w, parent)
            }
        }
        _ => false,
    };
    ensure!(ready, "Task parent does not satisfy its configured gate");
    Ok(())
}

pub fn claimable(w: &Workspace, actor: &str, key: &str, t: &Task) -> Result<()> {
    ensure!(
        w.members.contains_key(actor),
        "claimant is not a workspace member"
    );
    ensure!(t.assignee.is_none(), "task is already claimed");
    ensure!(t.done.is_none(), "task is already completed");
    ensure!(t.retired.is_none(), "task is retired");
    ensure!(
        !review_required(w, key, t),
        "task needs owner review after its parent revision changed"
    );
    ensure!(
        !t.contract.trim().is_empty(),
        "Task needs a technical contract before claim"
    );
    ensure!(
        objects::work_ready(w, key),
        "Task has an unmet dependency or a changed dependency revision"
    );
    check_projects(
        w,
        w.objects[key].current().parent.as_deref(),
        &subjects(w, key)?,
    )?;
    parent_ready(w, actor, key, &t.purpose)?;
    if t.purpose == "decision" {
        commands::human(w, actor)?;
        ensure!(
            w.objects.get(key).context("unknown Task")?.current().data["owner"] == actor,
            "designated Task owner must claim a decision task"
        );
    }
    Ok(())
}

pub fn subjects(w: &Workspace, key: &str) -> Result<Vec<String>> {
    if !w.schema_migrated {
        return Ok(task(w, key).map(|t| t.projects.clone()).unwrap_or_default());
    }
    let mut current = Some(key);
    let mut seen = std::collections::BTreeSet::new();
    let mut subjects = vec![];
    while let Some(key) = current {
        let key = objects::resolve_address(w, key);
        ensure!(seen.insert(key), "tree parent cycle");
        let o = w.objects.get(key).context("unknown subject context")?;
        subjects.extend(
            objects::reference_targets(w, &o.kind, &o.current().data)?
                .into_iter()
                .filter(|(_, _, subject)| *subject)
                .map(|(_, target, _)| target.split_once(':').unwrap().1.to_owned()),
        );
        current = o.current().parent.as_deref();
    }
    subjects.sort();
    subjects.dedup();
    Ok(subjects)
}

fn check_projects(w: &Workspace, parent: Option<&str>, projects: &[String]) -> Result<()> {
    let mut parent = parent;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(key) = parent {
        ensure!(seen.insert(key), "tree parent cycle");
        let o = w.objects.get(key).context("unknown Task ancestor")?;
        if let Some((_, r)) = objects::verification_context(w, key)
            && let Some(scope) = &r.verification_scope
        {
            ensure!(
                projects.iter().all(|p| scope.subject_keys.contains(p)),
                "Task reference is outside the agreed verification subject"
            );
        }
        parent = o.current().parent.as_deref();
    }
    Ok(())
}

pub fn save(
    w: &mut Workspace,
    actor: &str,
    key: &str,
    data: &Value,
    parent: Option<&str>,
    position: u64,
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
                || w.schema_migrated && !objects::process_field(field) && field != "projects"
                || w.schema_migrated
                    && rules.references.contains_key(field)
                    && !objects::process_field(field)
                || field == "projects"
                    && (!w.schema_migrated || data[field].as_array().is_some_and(Vec::is_empty)),
            "unknown Task document field {field}; use process commands for evidence"
        );
    }
    let mut t = task(w, key).cloned().unwrap_or_else(|| Task {
        title: String::new(),
        acs: vec![],
        projects: vec![],
        purpose: purpose.clone(),
        assignee: None,
        done: None,
        contract: String::new(),
        created_at: position,
        created_by: actor.into(),
        review_required: false,
        origin_revision: 1,
        retired: None,
    });
    if let Some(old) = old {
        ensure!(
            t.done.is_none() && t.retired.is_none(),
            "completed or retired Task cannot be amended"
        );
        ensure!(
            old.current().data["owner"] == actor
                || t.assignee.as_deref() == Some(actor)
                || t.assignee.is_none() && t.created_by == actor
                || actor == w.admin,
            "Task owner, assignee or unclaimed creator required"
        );
        ensure!(t.purpose == purpose, "task purpose cannot change");
    }
    t.title = string("title")?.into();
    t.contract = string("contract")?.into();
    t.projects = projects.clone();
    t.acs.clear();
    data["owner"] = json!(owner);
    if !w.schema_migrated {
        data["projects"] = json!(projects);
    }
    t.origin_revision = old.map_or(1, |o| o.current().revision + 1);
    if old.is_none() {
        let pin = parent.and_then(|p| {
            w.objects.get(p).map(|o| {
                (
                    p.to_owned(),
                    w.task_process
                        .get(p)
                        .map_or(o.current().revision, |t| t.origin_revision),
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
    Some(p)
}

pub fn apply(
    w: &mut Workspace,
    actor: &str,
    cmd: &Command,
    position: u64,
    at: &str,
) -> Result<bool> {
    let (req, revision, id) = match cmd {
        Command::ClaimTask { req, revision, id }
        | Command::AmendTask {
            req, revision, id, ..
        }
        | Command::ReleaseTask {
            req, revision, id, ..
        }
        | Command::CompleteTask {
            req, revision, id, ..
        }
        | Command::ReviewTask {
            req, revision, id, ..
        }
        | Command::RetireTask {
            req, revision, id, ..
        } => (req, *revision, id),
        _ => return Ok(false),
    };
    let key = address(req, id);
    if !w.task_process.contains_key(&key) {
        ensure!(!req.is_empty(), "use the Task queue address and revision");
        return Ok(false);
    }
    ensure!(
        req.is_empty(),
        "use the Task object address and revision without req"
    );
    let o = w.objects.get(&key).context("unknown Task")?;
    ensure!(o.current().present, "Task is absent from the current tree");
    ensure!(
        o.current().revision == revision,
        "stale Task revision; read the Task again"
    );
    let owner = o.current().data["owner"]
        .as_str()
        .context("Task needs an owner")?;
    let mut t = w.task_process[&key].clone();
    let authority = owner == actor || actor == w.admin;
    let reason = match cmd {
        Command::ClaimTask { .. } => {
            claimable(w, actor, &key, &t)?;
            t.assignee = Some(actor.into());
            "Task claimed"
        }
        Command::ReleaseTask { reason, .. } => {
            commands::nonempty(reason)?;
            ensure!(
                authority || t.assignee.as_deref() == Some(actor),
                "Task owner or assignee required"
            );
            ensure!(
                t.assignee.is_some() && t.done.is_none() && t.retired.is_none(),
                "Task is not claimed unfinished work"
            );
            t.assignee = None;
            reason
        }
        Command::CompleteTask { evidence, .. } => {
            commands::nonempty(evidence)?;
            ensure!(
                authority || t.assignee.as_deref() == Some(actor),
                "Task owner or assignee required"
            );
            ensure!(
                t.assignee.is_some()
                    && t.done.is_none()
                    && t.retired.is_none()
                    && !review_required(w, &key, &t),
                "Task must be claimed, unfinished and reviewed"
            );
            ensure!(
                objects::work_ready(w, &key),
                "Task has an unmet dependency or a changed dependency revision"
            );
            check_projects(w, o.current().parent.as_deref(), &subjects(w, &key)?)?;
            parent_ready(w, actor, &key, &t.purpose)?;
            if t.purpose == "decision"
                && (!w.schema_migrated
                    && objects::schema(w).types[&o.kind].parent_gate.as_deref() == Some("ready")
                    || objects::schema(w).types[&o.kind].parent_decision)
            {
                let parent = o
                    .current()
                    .parent
                    .as_deref()
                    .context("decision task requires a parent")?;
                let (req, r, ac) = objects::criterion_context(w, parent).context(
                    "configured parent decision requires a verification evidence provider",
                )?;
                let d = w.requirements[req]
                    .comments
                    .get(evidence)
                    .context("decision task evidence must be a comment id")?;
                ensure!(
                    d.kind == "decision"
                        && !d.withdrawn
                        && d.revision == r.number
                        && d.ac.as_deref() == Some(&ac.id),
                    "link the decision for this parent and revision"
                );
                ensure!(
                    ac.owner.as_ref().is_none_or(|owner| owner == &d.actor),
                    "designated specialist must decide"
                );
            }
            t.done = Some(evidence.clone());
            "Task completed"
        }
        Command::AmendTask {
            title,
            contract,
            projects,
            reason,
            ..
        } => {
            commands::nonempty(reason)?;
            let mut data = o.current().data.clone();
            data["title"] = json!(title);
            data["contract"] = json!(contract);
            if !w.schema_migrated
                || objects::schema(w).types[&o.kind]
                    .references
                    .contains_key("projects")
            {
                data["projects"] = json!(projects);
            } else {
                ensure!(
                    projects.is_empty(),
                    "configure a reference field before assigning projects"
                );
            }
            let parent = o.current().parent.clone();
            let data = save(w, actor, &key, &data, parent.as_deref(), position)?;
            let mut r = w.objects[&key].current().clone();
            r.data = data;
            append(w, &key, r, actor, position, at, reason);
            return Ok(true);
        }
        Command::ReviewTask { ac, reason, .. } => {
            commands::nonempty(reason)?;
            ensure!(
                ac.is_empty(),
                "generic Task review does not bind an AC; use save_object parent"
            );
            ensure!(
                authority && t.retired.is_none(),
                "active Task owner required"
            );
            check_projects(w, o.current().parent.as_deref(), &subjects(w, &key)?)?;
            t.review_required = false;
            let pin = context(w, &key);
            w.task_context.insert(key.clone(), pin);
            reason
        }
        Command::RetireTask { reason, .. } => {
            commands::nonempty(reason)?;
            ensure!(
                authority && t.retired.is_none(),
                "active Task owner required"
            );
            t.review_required = false;
            t.retired = Some(reason.clone());
            reason
        }
        _ => unreachable!(),
    };
    let r = w.objects[&key].current().clone();
    w.task_process.insert(key.clone(), t);
    append(w, &key, r, actor, position, at, reason);
    Ok(true)
}

fn append(
    w: &mut Workspace,
    key: &str,
    mut r: objects::ObjectRevision,
    actor: &str,
    position: u64,
    at: &str,
    reason: &str,
) {
    r.revision += 1;
    r.schema_revision = objects::schema(w).revision;
    r.actor = actor.into();
    r.at = at.into();
    r.position = position;
    r.reason = reason.into();
    w.objects.get_mut(key).unwrap().revisions.push(r);
}

pub fn migrate(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.tasks_migrated {
        return Ok(());
    }
    let mut candidate = w.clone();
    let legacy: Vec<(String, Task)> = w
        .requirements
        .iter()
        .flat_map(|(req, q)| {
            q.revisions.last().into_iter().flat_map(move |r| {
                r.tasks
                    .iter()
                    .map(move |(id, t)| (format!("task:{req}/{id}"), t.clone()))
            })
        })
        .collect();
    for (key, mut t) in legacy {
        if candidate.task_process.contains_key(&key) {
            continue;
        }
        let o = candidate
            .objects
            .get(&key)
            .context("legacy Task has no document")?;
        let mut r = o.current().clone();
        if t.purpose == "decision"
            && let Some(parent) = r.parent.as_deref().and_then(|p| w.objects.get(p))
            && parent.kind == "ac"
        {
            r.data["owner"] = parent.current().data["owner"].clone();
        }
        t.origin_revision = r.revision + 1;
        candidate
            .task_context
            .insert(key.clone(), context(&candidate, &key));
        candidate.task_process.insert(key.clone(), t);
        append(
            &mut candidate,
            &key,
            r,
            actor,
            position,
            at,
            "Give the Task an independent process",
        );
    }
    let mut schema = objects::schema(&candidate);
    if let Some(t) = schema.types.get_mut("task")
        && t.parents == vec!["ac"]
        && t.parent_gate.is_none()
    {
        t.parent_gate = Some("ready".into());
        objects::set_schema(
            &mut candidate,
            actor,
            schema.revision,
            &schema.types,
            "Record the default Task readiness gate",
            position,
            at,
        )?;
    }
    candidate.tasks_migrated = true;
    for key in candidate.task_process.keys().cloned().collect::<Vec<_>>() {
        candidate
            .task_context
            .insert(key.clone(), context(&candidate, &key));
    }
    *w = candidate;
    Ok(())
}
