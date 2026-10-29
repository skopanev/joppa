//! Part of `tasks`: `apply` and the items after it, moved from tasks.rs unchanged.
use super::*;

pub fn apply(
    w: &mut Workspace,
    actor: &str,
    cmd: &Command,
    position: u64,
    at: &str,
) -> Result<bool> {
    let (req, revision, id) = match cmd {
        Command::HeartbeatTask { revision, id }
        | Command::BlockTask { revision, id, .. }
        | Command::UnblockTask { revision, id, .. } => (&String::new(), *revision, id),
        Command::ClaimTask { req, revision, id }
        | Command::AmendTask(crate::commands::AmendTaskOp {
            req, revision, id, ..
        })
        | Command::ReleaseTask {
            req, revision, id, ..
        }
        | Command::CompleteTask(crate::commands::CompleteTaskOp {
            req, revision, id, ..
        })
        | Command::ReviewTask(crate::commands::ReviewTaskOp {
            req, revision, id, ..
        })
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
    // A lost race names the claim, not the revision that the claim moved.
    if matches!(cmd, Command::ClaimTask { .. }) {
        ensure!(
            holder(w, &w.task_process[&key]).is_none(),
            "task is already claimed"
        );
    }
    ensure!(
        o.current().revision == revision,
        "stale Task revision; read the Task again"
    );
    let owner = o.current().data["owner"]
        .as_str()
        .context("Task needs an owner")?;
    let mut t = w.task_process[&key].clone();
    let authority = owner == actor || actor == w.admin;
    if expired(w, &t) {
        t.assignee = None;
    }
    let reason = match cmd {
        Command::HeartbeatTask { .. } => {
            ensure!(
                t.assignee.as_deref() == Some(actor) && t.done.is_none() && t.retired.is_none(),
                "only the holder can renew an unexpired claim"
            );
            let now = w.claim_time_ms.context("server claim time is missing")?;
            t.claim = Some(crate::model::TaskClaim {
                heartbeat_at_ms: now,
                deadline_ms: now + CLAIM_DURATION_MS,
            });
            w.task_process.insert(key, t);
            return Ok(true);
        }
        Command::ClaimTask { .. } => {
            validate_claim(w, actor, &key, &t)?;
            t.assignee = Some(actor.into());
            t.claim = if w.claims_migrated {
                let now = w.claim_time_ms.context("server claim time is missing")?;
                Some(crate::model::TaskClaim {
                    heartbeat_at_ms: now,
                    deadline_ms: now + CLAIM_DURATION_MS,
                })
            } else {
                None
            };
            "Task claimed"
        }
        Command::ReleaseTask { reason, .. } => {
            commands::nonempty(reason)?;
            release_authority(w, actor, &t, authority)?;
            ensure!(t.blocked.is_none(), BLOCKED);
            ensure!(
                t.assignee.is_some() && t.done.is_none() && t.retired.is_none(),
                "Task is not claimed unfinished work"
            );
            t.assignee = None;
            t.claim = None;
            reason
        }
        Command::BlockTask {
            reason, evidence, ..
        } => {
            commands::nonempty(reason)?;
            commands::nonempty(evidence)?;
            let legacy = w.replay_claims && !w.persistent_claims && authority;
            ensure!(
                t.assignee.is_some() || actor == w.admin || legacy,
                "Task is not claimed unfinished work"
            );
            ensure!(
                actor == w.admin || t.assignee.as_deref() == Some(actor) || legacy,
                "Task holder required"
            );
            ensure!(t.done.is_none(), "completed Task cannot be blocked");
            ensure!(t.retired.is_none(), "retired Task cannot be blocked");
            ensure!(t.blocked.is_none(), "Task is already blocked");
            t.assignee = None;
            t.claim = None;
            t.blocked = Some(crate::model::TaskBlock {
                actor: actor.into(),
                reason: reason.clone(),
                evidence: evidence.clone(),
                position,
            });
            reason
        }
        Command::UnblockTask {
            reason, evidence, ..
        } => {
            commands::nonempty(reason)?;
            commands::nonempty(evidence)?;
            ensure!(t.retired.is_none(), "retired Task cannot be unblocked");
            ensure!(t.blocked.is_some(), "Task is not blocked");
            ensure!(
                actor == w.admin || parent_planner(w, o, actor),
                "planning holder or workspace owner required"
            );
            t.blocked = None;
            reason
        }
        Command::CompleteTask(crate::commands::CompleteTaskOp {
            evidence, commit, ..
        }) => {
            completion::check(
                w,
                actor,
                &key,
                o,
                &t,
                authority,
                evidence,
                commit.as_deref(),
            )?;
            t.done = Some(evidence.clone());
            t.commit = commit.clone();
            t.claim = None;
            "Task completed"
        }
        Command::AmendTask(crate::commands::AmendTaskOp {
            title,
            contract,
            projects,
            reason,
            ..
        }) => {
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
            let data = save(w, actor, &key, &data, parent.as_deref(), position, |_| {
                false
            })?;
            let mut r = w.objects[&key].current().clone();
            r.data = data;
            append(w, &key, r, actor, position, at, reason);
            return Ok(true);
        }
        Command::ReviewTask(crate::commands::ReviewTaskOp { ac, reason, .. }) => {
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
            // The planning holder of the parent criterion may retire a blocked child Task.
            let planner = t.blocked.is_some() && parent_planner(w, o, actor);
            ensure!(
                (authority || planner) && t.retired.is_none(),
                "active Task owner required"
            );
            t.review_required = false;
            t.retired = Some(reason.clone());
            t.claim = None;
            reason
        }
        _ => unreachable!(),
    };
    let r = w.objects[&key].current().clone();
    w.task_process.insert(key.clone(), t);
    append(w, &key, r, actor, position, at, reason);
    Ok(true)
}

/// The actor holds the planning claim on the parent criterion of the Task.
pub(crate) fn parent_planner(w: &Workspace, o: &objects::Object, actor: &str) -> bool {
    o.current().parent.as_deref().is_some_and(|p| {
        w.planning_claims
            .get(objects::resolve_address(w, p))
            .is_some_and(|c| c.actor == actor)
    })
}
