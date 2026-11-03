//! The replay of the older Task operations. Live writes refuse them (removed_ops), and each
//! stored event passed its checks when it was written, so the replay only records its effect.
//! migrate_generic_nodes then moves the holder, the commit and the end of each Task into the
//! node itself.
use super::*;

pub fn apply(
    w: &mut Workspace,
    actor: &str,
    cmd: &Command,
    position: u64,
    at: &str,
) -> Result<bool> {
    let (req, id) = match cmd {
        Command::HeartbeatTask { id, .. }
        | Command::BlockTask { id, .. }
        | Command::UnblockTask { id, .. } => ("", id),
        Command::ClaimTask { req, id, .. }
        | Command::AmendTask(crate::commands::AmendTaskOp { req, id, .. })
        | Command::ReleaseTask { req, id, .. }
        | Command::CompleteTask(crate::commands::CompleteTaskOp { req, id, .. })
        | Command::ReviewTask(crate::commands::ReviewTaskOp { req, id, .. })
        | Command::RetireTask { req, id, .. } => (req.as_str(), id),
        _ => return Ok(false),
    };
    let key = address(req, id);
    // A Task of an older requirement revision replays in checks.
    let Some(mut t) = w.task_process.get(&key).cloned() else {
        return Ok(false);
    };
    if expired(w, &t) {
        t.assignee = None;
    }
    let claim = |w: &Workspace| {
        w.claim_time_ms.map(|now| crate::model::TaskClaim {
            heartbeat_at_ms: now,
            deadline_ms: now + CLAIM_DURATION_MS,
        })
    };
    let reason = match cmd {
        Command::HeartbeatTask { .. } => {
            t.claim = claim(w);
            w.task_process.insert(key, t);
            return Ok(true);
        }
        Command::ClaimTask { .. } => {
            t.assignee = Some(actor.into());
            t.claim = if w.claims_migrated { claim(w) } else { None };
            "Task claimed"
        }
        Command::ReleaseTask { reason, .. } => {
            t.assignee = None;
            t.claim = None;
            reason
        }
        Command::BlockTask {
            reason, evidence, ..
        } => {
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
        Command::UnblockTask { reason, .. } => {
            t.blocked = None;
            reason
        }
        Command::CompleteTask(crate::commands::CompleteTaskOp {
            evidence, commit, ..
        }) => {
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
            let o = w.objects.get(&key).context("unknown Task")?;
            let mut data = o.current().data.clone();
            data["title"] = json!(title);
            data[crate::body_migration::text_field(w, "contract")] = json!(contract);
            let references = &objects::schema(w).types[&o.kind].references;
            if !w.schema_migrated || references.contains_key("projects") {
                data["projects"] = json!(projects);
            }
            let parent = o.current().parent.clone();
            let data = save(w, actor, &key, &data, parent.as_deref(), position)?;
            let mut r = w.objects[&key].current().clone();
            r.data = data;
            append(w, &key, r, actor, position, at, reason);
            return Ok(true);
        }
        Command::ReviewTask(crate::commands::ReviewTaskOp { reason, .. }) => {
            t.review_required = false;
            let pin = context(w, &key);
            w.task_context.insert(key.clone(), pin);
            reason
        }
        Command::RetireTask { reason, .. } => {
            t.review_required = false;
            t.retired = Some(reason.clone());
            t.claim = None;
            reason
        }
        _ => unreachable!(),
    };
    let r = w
        .objects
        .get(&key)
        .context("unknown Task")?
        .current()
        .clone();
    w.task_process.insert(key.clone(), t);
    append(w, &key, r, actor, position, at, reason);
    Ok(true)
}
