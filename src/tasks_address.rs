//! The address, the holder and the review of a Task, as the replay of the older checks reads
//! them.
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

pub(crate) fn parse_time(at: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(at)
        .or_else(|_| chrono::DateTime::parse_from_str(at, "%Y-%m-%d %H:%M:%S%.f%#z"))
        .ok()
        .map(|time| time.timestamp_millis())
}

pub(crate) fn expired(w: &Workspace, t: &Task) -> bool {
    w.replaying
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

pub fn readiness_blocker(w: &Workspace, key: &str) -> Option<Value> {
    let o = w.objects.get(key)?;
    let schema = objects::schema(w);
    let t = schema.types.get(&o.kind)?;
    if t.parent_gate.as_deref() != Some("ready") {
        return None;
    }
    objects::readiness_blocker(w, o.current().parent.as_deref()?, true)
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

/// The older rule of the legacy claim path: an agent may hold several Tasks only when they all
/// belong to the REQ of the candidate. Current claims do not apply it.
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
