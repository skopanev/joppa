//! The replay of the older planning claims of a criterion. Live writes refuse these operations
//! (removed_ops). A release still retires each Task that the holder made during the claim.
use crate::{
    commands::Command,
    model::{PlanningClaim, Workspace},
    objects, tasks,
};
use anyhow::{Context, Result};

pub fn apply(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
) -> Result<bool> {
    let ac = match command {
        Command::ClaimPlanning { ac, .. }
        | Command::FinishPlanning { ac, .. }
        | Command::ReleasePlanning { ac, .. } => ac,
        _ => return Ok(false),
    };
    let key = objects::resolve_address(w, ac).to_owned();
    if let Command::ClaimPlanning { .. } = command {
        let actor = actor.into();
        w.planning_claims
            .insert(key, PlanningClaim { actor, position });
        return Ok(true);
    }
    let claim = w
        .planning_claims
        .remove(&key)
        .context("criterion has no planning claim")?;
    let Command::ReleasePlanning { reason, .. } = command else {
        return Ok(true);
    };
    let made: Vec<String> = w
        .task_process
        .iter()
        .filter(|(k, t)| {
            t.created_by == claim.actor
                && t.created_at > claim.position
                && w.objects.get(*k).is_some_and(|o| {
                    o.current().present
                        && o.current()
                            .parent
                            .as_deref()
                            .map(|p| objects::resolve_address(w, p))
                            == Some(key.as_str())
                })
        })
        .map(|(k, _)| k.clone())
        .collect();
    for k in made {
        let t = w.task_process.get_mut(&k).expect("listed Task");
        t.retired = Some(reason.clone());
        t.assignee = None;
        t.claim = None;
        t.review_required = false;
        let revision = w.objects[&k].current().clone();
        tasks::append(w, &k, revision, actor, position, at, reason);
    }
    Ok(true)
}
