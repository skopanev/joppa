//! Part of `tasks`: `address` and the items after it, moved from tasks.rs unchanged.
use super::*;

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

pub const CLAIM_DURATION_MS: i64 = 30 * 60 * 1000;

pub const BLOCKED: &str = "Task is blocked for planning";

pub(crate) fn parse_time(at: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(at)
        .or_else(|_| chrono::DateTime::parse_from_str(at, "%Y-%m-%d %H:%M:%S%.f%#z"))
        .ok()
        .map(|time| time.timestamp_millis())
}

pub(crate) fn expired(w: &Workspace, t: &Task) -> bool {
    w.replay_claims
        && !w.persistent_claims
        && t.done.is_none()
        && t.retired.is_none()
        && t.claim
            .as_ref()
            .is_some_and(|claim| w.claim_time_ms.is_some_and(|now| now >= claim.deadline_ms))
}

pub(crate) fn holder<'a>(w: &Workspace, t: &'a Task) -> Option<&'a str> {
    if expired(w, t) {
        None
    } else {
        t.assignee.as_deref()
    }
}

pub(crate) fn unclaimed_eligibility(w: &Workspace, actor: &str, key: &str, t: &Task) -> Result<()> {
    let mut open = t.clone();
    open.assignee = None;
    open.claim = None;
    validate_claim(w, actor, key, &open)
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
        if !objects::within(w, key, &objects::requirement_address(req)) {
            continue;
        }
        let mut t = t.clone();
        t.review_required = review_required(w, key, &t);
        t.acs = w.objects[key]
            .current()
            .parent
            .as_deref()
            .and_then(|p| objects::criterion_context(w, p).map(|(_, _, ac)| vec![ac.id.clone()]))
            .unwrap_or_default();
        let id = key.strip_prefix(&format!("task:{req}/")).unwrap_or(key);
        result.insert(id.into(), t);
    }
    result
}

pub(crate) fn context(w: &Workspace, key: &str) -> Option<(String, u32)> {
    let p = objects::resolve_address(w, w.objects.get(key)?.current().parent.as_deref()?);
    Some((
        p.to_owned(),
        w.task_process
            .get(p)
            .map_or(objects::contract_revision(w.objects.get(p)?), |t| {
                t.origin_revision
            }),
    ))
}

/// A done Task never asks for a review; a new text of its parent does not reopen it.
pub fn review_required(w: &Workspace, key: &str, t: &Task) -> bool {
    t.done.is_none()
        && (t.review_required
            || w.task_context
                .get(key)
                .is_some_and(|old| old != &context(w, key)))
}

pub fn parent_assessment(w: &Workspace, key: &str) -> Option<Value> {
    objects::assessment(w, w.objects.get(key)?.current().parent.as_deref()?)
}

pub fn readiness_blocker(w: &Workspace, key: &str) -> Option<Value> {
    let o = w.objects.get(key)?;
    let schema = objects::schema(w);
    let t = schema.types.get(&o.kind)?;
    if t.parent_gate.as_deref() != Some("ready") {
        return None;
    }
    let decision = task(w, key)?.purpose == "decision" && (!w.schema_migrated || t.parent_decision);
    objects::readiness_blocker(w, o.current().parent.as_deref()?, !decision)
}

pub(crate) fn parent_ready(w: &Workspace, actor: &str, key: &str, purpose: &str) -> Result<()> {
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
    let process = objects::run_process(w, parent);
    let ready = match gate {
        "completed" => task(w, parent).is_some_and(|t| t.done.is_some() && t.retired.is_none()),
        "verified" => process["verification"] == "pass",
        "ready" => {
            if objects::criterion_context(w, parent).is_some() {
                if purpose == "decision"
                    && (!w.schema_migrated || schema.types[&o.kind].parent_decision)
                {
                    commands::human(w, actor)?;
                }
                readiness_blocker(w, key).is_none()
            } else if objects::input_type(w, &w.objects[parent].kind) {
                // A Task under a signal needs the signal marked ready for work.
                match w.signal_assessments.get(parent) {
                    None => anyhow::bail!("{parent} has no readiness assessment"),
                    Some(a) if !a.ready => {
                        anyhow::bail!("{parent} is not ready for work: {}", a.reason)
                    }
                    Some(_) => objects::work_ready(w, parent),
                }
            } else {
                objects::work_ready(w, parent)
            }
        }
        _ => false,
    };
    if !ready && let Some(blocker) = readiness_blocker(w, key) {
        anyhow::bail!("{}", objects::assessment_reason(&blocker));
    }
    ensure!(ready, "Task parent does not satisfy its configured gate");
    Ok(())
}

/// The REQ above a Task, found through its configured parents.
pub(crate) fn requirement(w: &Workspace, key: &str) -> Option<String> {
    let types = objects::schema(w).types;
    let mut current = Some(objects::resolve_address(w, key));
    let mut seen = std::collections::BTreeSet::new();
    while let Some(key) = current {
        if !seen.insert(key) {
            return None;
        }
        let o = w.objects.get(key)?;
        if types.get(&o.kind).is_some_and(|t| t.role == "requirement") {
            return Some(key.into());
        }
        current = o
            .current()
            .parent
            .as_deref()
            .map(|parent| objects::resolve_address(w, parent));
    }
    None
}

/// An agent may hold several Tasks only when they all belong to the REQ of the candidate.
pub(crate) fn ensure_agent_available(w: &Workspace, actor: &str, candidate: &str) -> Result<()> {
    if w.members.get(actor).is_none_or(|role| role != "agent") {
        return Ok(());
    }
    let req = requirement(w, candidate);
    let active = |t: &Task| holder(w, t) == Some(actor) && t.done.is_none() && t.retired.is_none();
    let conflict = |key: &str| req.is_none() || requirement(w, key) != req;
    if let Some((key, _)) = w.task_process.iter().find(|(key, t)| {
        active(t) && w.objects.get(*key).is_some_and(|o| o.current().present) && conflict(key)
    }) {
        anyhow::bail!("agent already holds task {key}");
    }
    for (req, requirement) in &w.requirements {
        if let Some(revision) = requirement.revisions.last() {
            for (id, task) in &revision.tasks {
                let key = address(req, id);
                if !w.task_process.contains_key(&key) && active(task) && conflict(&key) {
                    anyhow::bail!("agent already holds task {key}");
                }
            }
        }
    }
    Ok(())
}

pub fn claimable(w: &Workspace, actor: &str, key: &str, t: &Task) -> Result<()> {
    validate_claim(w, actor, key, t)?;
    ensure_agent_available(w, actor, key)
}
