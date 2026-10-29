//! Arms of `apply` in checks.rs, one function for each operation.
use super::*;

/// Apply `task`.
pub(super) fn apply_task(
    w: &mut Workspace,
    actor: &str,
    cmd: &Command,
    position: u64,
) -> Result<()> {
    let Command::Task(crate::commands::TaskOp {
        req,
        revision: n,
        id,
        title,
        acs,
        projects,
        purpose,
        assignee,
        contract,
    }) = cmd
    else {
        unreachable!()
    };
    identifier(id)?;
    nonempty(title)?;
    if let Some(assignee) = assignee {
        ensure!(w.members.contains_key(assignee), "unknown assignee");
    } else {
        nonempty(contract)?;
    }
    ensure!(!purpose.trim().is_empty(), "task purpose must be nonempty");
    for project in projects {
        ensure!(w.projects.contains_key(project), "unknown project");
    }
    let r = revision(w, req, *n, true)?;
    ensure!(!r.tasks.contains_key(id), "task exists");
    if let Some(contract) = &r.verification_scope {
        for project in projects {
            ensure!(
                contract.subject_keys.contains(project),
                "task project is outside the agreed verification subject"
            );
        }
    }
    ensure!(!acs.is_empty(), "task needs at least one AC");
    for ac in acs {
        ensure!(r.acs.iter().any(|a| &a.id == ac), "unknown AC");
        if assignee.is_some() && purpose != "decision" {
            ensure!(ready(r, ac), "AC awaits decision or dependent AC");
        }
    }
    r.tasks.insert(
        id.clone(),
        Task {
            claim: None,
            title: title.clone(),
            acs: acs.clone(),
            projects: projects.clone(),
            purpose: purpose.clone(),
            assignee: assignee.clone(),
            done: None,
            commit: None,
            contract: contract.clone(),
            created_at: position,
            created_by: actor.into(),
            review_required: false,
            origin_revision: *n,
            retired: None,
            blocked: None,
        },
    );
    Ok(())
}

/// Apply `claim_task`.
pub(super) fn apply_claim_task(w: &mut Workspace, actor: &str, cmd: &Command) -> Result<()> {
    let Command::ClaimTask {
        req,
        revision: n,
        id,
    } = cmd
    else {
        unreachable!()
    };
    let q = w
        .requirements
        .get(req)
        .ok_or_else(|| anyhow::anyhow!("unknown REQ"))?;
    ensure!(
        q.revisions.last().is_some_and(|r| r.number == *n) && *n > 0,
        "stale revision; read the task again"
    );
    let r = q.revisions.last().unwrap();
    let task = r
        .tasks
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("unknown task"))?;
    validate_claim(w, actor, req, id, r, task)?;
    revision(w, req, *n, true)?
        .tasks
        .get_mut(id)
        .unwrap()
        .assignee = Some(actor.into());
    Ok(())
}

/// Apply `amend_task`.
pub(super) fn apply_amend_task(w: &mut Workspace, actor: &str, cmd: &Command) -> Result<()> {
    let Command::AmendTask(crate::commands::AmendTaskOp {
        req,
        revision: n,
        id,
        title,
        contract,
        projects,
        reason,
    }) = cmd
    else {
        unreachable!()
    };
    nonempty(title)?;
    nonempty(contract)?;
    nonempty(reason)?;
    for project in projects {
        ensure!(w.projects.contains_key(project), "unknown project");
    }
    let r = revision(w, req, *n, true)?;
    let task = r
        .tasks
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("unknown task"))?;
    ensure!(task.done.is_none(), "completed task cannot be amended");
    ensure!(task.retired.is_none(), "retired task cannot be amended");
    ensure!(
        actor == r.owner
            || task.assignee.as_deref() == Some(actor)
            || (task.assignee.is_none() && task.created_by == actor),
        "REQ owner, assignee or unclaimed task creator required"
    );
    if let Some(scope) = &r.verification_scope {
        for project in projects {
            ensure!(
                scope.subject_keys.contains(project),
                "task project is outside the agreed verification subject"
            );
        }
    }
    task.title = title.clone();
    task.contract = contract.clone();
    task.projects = projects.clone();
    Ok(())
}

/// Apply `release_task`.
pub(super) fn apply_release_task(w: &mut Workspace, actor: &str, cmd: &Command) -> Result<()> {
    let Command::ReleaseTask {
        req,
        revision: n,
        id,
        reason,
    } = cmd
    else {
        unreachable!()
    };
    nonempty(reason)?;
    let r = revision(w, req, *n, true)?;
    let task = r
        .tasks
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("unknown task"))?;
    ensure!(
        task.assignee.as_deref() == Some(actor) || r.owner == actor,
        "assignee or REQ owner releases a task"
    );
    ensure!(task.assignee.is_some(), "task is already unassigned");
    ensure!(task.done.is_none(), "completed task cannot be released");
    ensure!(task.retired.is_none(), "retired task cannot be released");
    ensure!(
        !task.contract.trim().is_empty(),
        "add a technical contract before releasing this legacy task"
    );
    task.assignee = None;
    Ok(())
}

/// Apply `review_task`.
pub(super) fn apply_review_task(w: &mut Workspace, actor: &str, cmd: &Command) -> Result<()> {
    let Command::ReviewTask(crate::commands::ReviewTaskOp {
        req,
        revision: n,
        id,
        ac,
        reason,
    }) = cmd
    else {
        unreachable!()
    };
    nonempty(reason)?;
    let r = revision(w, req, *n, true)?;
    ensure!(r.owner == actor, "REQ owner reviews carried tasks");
    ensure!(
        r.acs.iter().any(|a| &a.id == ac),
        "unknown AC in current revision"
    );
    let task = r
        .tasks
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("unknown task"))?;
    ensure!(
        task.review_required,
        "task already reviewed for this revision"
    );
    if let Some(contract) = &r.verification_scope {
        for project in &task.projects {
            ensure!(
                contract.subject_keys.contains(project),
                "task project is outside the agreed verification subject"
            );
        }
    }
    task.acs = vec![ac.clone()];
    task.review_required = false;
    Ok(())
}

/// Apply `retire_task`.
pub(super) fn apply_retire_task(w: &mut Workspace, actor: &str, cmd: &Command) -> Result<()> {
    let Command::RetireTask {
        req,
        revision: n,
        id,
        reason,
    } = cmd
    else {
        unreachable!()
    };
    nonempty(reason)?;
    let r = revision(w, req, *n, true)?;
    ensure!(r.owner == actor, "REQ owner retires carried tasks");
    let task = r
        .tasks
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("unknown task"))?;
    ensure!(task.retired.is_none(), "task is already retired");
    task.review_required = false;
    task.retired = Some(reason.clone());
    Ok(())
}
