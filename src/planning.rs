//! Planning ownership follows the recorded workspace journal.
use crate::{
    commands::{self, Command},
    model::{PlanningClaim, Task, Workspace},
    objects, tasks,
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::collections::BTreeSet;

fn criterion(w: &Workspace, ac: &str) -> Result<String> {
    let key = objects::resolve_address(w, ac);
    let object = w
        .objects
        .get(key)
        .filter(|o| o.current().present)
        .context("unknown criterion")?;
    ensure!(
        objects::schema(w)
            .types
            .get(&object.kind)
            .is_some_and(|t| t.role == "ac"),
        "planning requires a criterion"
    );
    Ok(key.into())
}

pub(crate) fn validate_claim(w: &Workspace, ac: &str, revision: u32) -> Result<String> {
    let key = criterion(w, ac)?;
    ensure!(
        !w.planning_claims.contains_key(&key),
        "criterion is already claimed for planning"
    );
    ensure!(
        w.objects[&key].current().revision == revision,
        "stale object revision; read the object again"
    );
    Ok(key)
}

fn child(w: &Workspace, key: &str, ac: &str) -> bool {
    w.objects.get(key).is_some_and(|o| {
        o.current().present
            && o.current()
                .parent
                .as_deref()
                .map(|p| objects::resolve_address(w, p))
                == Some(ac)
            && objects::schema(w)
                .types
                .get(&o.kind)
                .is_some_and(|t| t.role == "task")
    })
}

fn new_task(t: &Task, claim: &PlanningClaim) -> bool {
    t.created_by == claim.actor && t.created_at > claim.position
}

pub fn awaiting_publication(w: &Workspace, key: &str, t: &Task) -> bool {
    let Some(parent) = w
        .objects
        .get(key)
        .and_then(|o| o.current().parent.as_deref())
    else {
        return false;
    };
    w.planning_claims
        .get(objects::resolve_address(w, parent))
        .is_some_and(|claim| new_task(t, claim))
}

/// Blocking links by relation, target and revision pin, in any order.
type Links<'a> = BTreeSet<(&'a str, &'a str, Option<u32>)>;

/// The Task contract: title, contract, projects and blocking links.
fn contract(r: &objects::ObjectRevision) -> (&Value, &Value, &Value, Links<'_>) {
    (
        &r.data["title"],
        &r.data["contract"],
        &r.data["projects"],
        r.links
            .iter()
            .filter(|l| l.blocking)
            .map(|l| (l.relation.as_str(), l.target.as_str(), l.revision))
            .collect(),
    )
}

/// A blocked Task leaves planning only with a new contract or by retirement.
fn validate_blocked(w: &Workspace, key: &str, position: u64) -> Result<()> {
    let o = &w.objects[key];
    let baseline = o
        .revisions
        .iter()
        .rev()
        .find(|r| r.position <= position)
        .context("blocked Task revision is missing")?;
    ensure!(
        contract(baseline) != contract(o.current()),
        "blocked Task needs a new contract or retirement"
    );
    Ok(())
}

fn validate_dependencies(w: &Workspace, roots: &[String]) -> Result<()> {
    fn visit(
        w: &Workspace,
        key: &str,
        stack: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        let key = objects::resolve_address(w, key);
        if done.contains(key) {
            return Ok(());
        }
        let object = w
            .objects
            .get(key)
            .filter(|o| o.current().present)
            .context("planning dependency target is missing")?;
        ensure!(
            stack.insert(key.into()),
            "planning dependencies contain a cycle"
        );
        for link in object
            .current()
            .links
            .iter()
            .filter(|l| l.relation == "depends_on")
        {
            visit(w, &link.target, stack, done)?;
        }
        stack.remove(key);
        done.insert(key.into());
        Ok(())
    }
    let mut done = BTreeSet::new();
    for key in roots {
        visit(w, key, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}

pub fn apply(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
) -> Result<bool> {
    let (ac, reason) = match command {
        Command::ClaimPlanning { ac, .. } => (ac, None),
        Command::FinishPlanning { ac, reason } | Command::ReleasePlanning { ac, reason } => {
            (ac, Some(reason))
        }
        _ => return Ok(false),
    };
    if let Some(reason) = reason {
        commands::nonempty(reason)?;
    }
    let key = criterion(w, ac)?;
    if let Command::ClaimPlanning { revision, .. } = command {
        validate_claim(w, ac, *revision)?;
        ensure!(
            objects::assessment(w, &key).is_some_and(|a| a["state"] == "ready")
                && objects::readiness_blocker(w, &key, true).is_none()
                && objects::review_links(w, &key).is_empty()
                && objects::work_ready(w, &key),
            "criterion is not ready for planning"
        );
        w.planning_claims.insert(
            key,
            PlanningClaim {
                actor: actor.into(),
                position,
            },
        );
        return Ok(true);
    }
    let claim = w
        .planning_claims
        .get(&key)
        .context("criterion has no planning claim")?
        .clone();
    let children: Vec<String> = w
        .task_process
        .keys()
        .filter(|k| child(w, k, &key))
        .cloned()
        .collect();
    match command {
        Command::FinishPlanning { .. } => {
            ensure!(claim.actor == actor, "planning holder required");
            ensure!(
                crate::remote_links::changed_since(w, &key, claim.position, true).is_none(),
                "the remote links changed after the claim; release and claim again"
            );
            let blocked: Vec<String> = children
                .iter()
                .filter(|k| {
                    let t = &w.task_process[*k];
                    t.blocked.is_some() && t.retired.is_none()
                })
                .cloned()
                .collect();
            for k in &blocked {
                validate_blocked(w, k, w.task_process[k].blocked.as_ref().unwrap().position)?;
            }
            // A holder's test-list save after the claim also counts: the plan binds existing tests.
            ensure!(
                children.iter().any(|k| {
                    new_task(&w.task_process[k], &claim)
                        || w.objects[k]
                            .revisions
                            .iter()
                            .any(|r| r.actor == actor && r.position > claim.position)
                }) || w.ac_tests.get(&key).is_some_and(|t| {
                    t.revisions
                        .iter()
                        .any(|r| r.actor == actor && r.position > claim.position)
                }),
                "planning produced no Task changes"
            );
            validate_dependencies(w, &children)?;
            for k in &blocked {
                w.task_process.get_mut(k).unwrap().blocked = None;
            }
            // A Task the holder saved during the claim follows the current criterion, as review_task does.
            let replanned: Vec<String> = children
                .iter()
                .filter(|k| {
                    w.task_process[*k].retired.is_none()
                        && w.objects[*k]
                            .revisions
                            .iter()
                            .any(|r| r.actor == actor && r.position > claim.position)
                })
                .cloned()
                .collect();
            for k in replanned {
                w.task_process.get_mut(&k).unwrap().review_required = false;
                let pin = tasks::context(w, &k);
                w.task_context.insert(k, pin);
            }
        }
        Command::ReleasePlanning { reason, .. } => {
            ensure!(
                claim.actor == actor || w.admin == actor,
                "workspace owner required"
            );
            for task_key in children
                .iter()
                .filter(|k| new_task(&w.task_process[*k], &claim))
                .cloned()
                .collect::<Vec<_>>()
            {
                let t = w.task_process.get_mut(&task_key).unwrap();
                t.retired = Some(reason.clone());
                t.assignee = None;
                t.claim = None;
                t.review_required = false;
                let revision = w.objects[&task_key].current().clone();
                tasks::append(w, &task_key, revision, actor, position, at, reason);
            }
        }
        _ => unreachable!(),
    }
    w.planning_claims.remove(&key);
    Ok(true)
}
