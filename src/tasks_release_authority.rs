//! Part of `tasks`: `release_authority` and the items after it, moved from tasks.rs unchanged.
use super::*;

/// The holder or the workspace owner may take a Task out of active work.
pub(crate) fn release_authority(
    w: &Workspace,
    actor: &str,
    t: &Task,
    authority: bool,
) -> Result<()> {
    ensure!(
        actor == w.admin
            || t.assignee.as_deref() == Some(actor)
            || w.replay_claims && !w.persistent_claims && authority,
        "workspace owner required"
    );
    Ok(())
}

pub(crate) fn append(
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
            None,
            None,
            None,
            None,
            None,
            objects::TASK_GATE_SCHEMA_REASON,
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
