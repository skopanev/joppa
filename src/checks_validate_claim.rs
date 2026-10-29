//! Part of `checks`: `validate_claim` and the items after it, moved from checks.rs unchanged.
use super::*;

// Existing journal claims can predate the limit on active work.
pub(crate) fn validate_claim(
    w: &Workspace,
    actor: &str,
    req: &str,
    id: &str,
    r: &Revision,
    task: &Task,
) -> Result<()> {
    ensure!(
        w.members.contains_key(actor),
        "claimant is not a workspace member"
    );
    ensure!(task.assignee.is_none(), "task is already claimed");
    ensure!(task.done.is_none(), "task is already completed");
    ensure!(task.retired.is_none(), "task is retired");
    ensure!(
        !task.review_required,
        "task needs owner review after REQ revision"
    );
    ensure!(
        !task.contract.trim().is_empty(),
        "task needs a technical contract before claim"
    );
    ensure!(task.acs.len() == 1, "task must belong to exactly one AC");
    let ac = r
        .acs
        .iter()
        .find(|ac| ac.id == task.acs[0])
        .ok_or_else(|| anyhow::anyhow!("task AC is absent from current REQ revision"))?;
    if !w.objects.is_empty() {
        ensure!(
            crate::objects::work_ready(w, &crate::objects::criterion_address(&ac.id)),
            "AC has an unmet dependency or a changed dependency revision"
        );
        ensure!(
            crate::objects::work_ready(w, &format!("task:{req}/{id}")),
            "Task has an unmet dependency or a changed dependency revision"
        );
    }
    if task.purpose == "decision" {
        ensure!(
            w.members[actor] == "human",
            "a human must claim a decision task"
        );
        ensure!(
            ac.owner.as_ref().unwrap_or(&r.owner) == actor,
            "designated AC owner must claim the decision task"
        );
        ensure!(
            ac.depends_on.iter().all(|id| ready(r, id)),
            "dependent AC is not ready"
        );
    } else {
        ensure!(ready(r, &ac.id), "AC is not ready for work");
    }
    Ok(())
}

pub fn apply(
    w: &mut Workspace,
    actor: &str,
    cmd: &Command,
    position: u64,
    _recorded_at: &str,
) -> Result<()> {
    match cmd {
        Command::Task(crate::commands::TaskOp { .. }) => apply_task(w, actor, cmd, position)?,
        Command::ClaimTask { .. } => apply_claim_task(w, actor, cmd)?,
        Command::AmendTask(crate::commands::AmendTaskOp { .. }) => apply_amend_task(w, actor, cmd)?,
        Command::ReleaseTask { .. } => apply_release_task(w, actor, cmd)?,
        Command::ReviewTask(crate::commands::ReviewTaskOp { .. }) => {
            apply_review_task(w, actor, cmd)?
        }
        Command::RetireTask { .. } => apply_retire_task(w, actor, cmd)?,
        Command::CompleteTask(crate::commands::CompleteTaskOp { .. }) => {
            apply_complete_task(w, actor, cmd)?
        }
        Command::DefineVerificationScope(crate::commands::DefineVerificationScopeOp { .. }) => {
            apply_define_verification_scope(w, actor, cmd, position)?
        }
        Command::Plan(crate::commands::PlanOp { .. }) => apply_plan(w, actor, cmd, position)?,
        Command::ReviewCoverage(crate::commands::ReviewCoverageOp { .. }) => {
            apply_review_coverage(w, actor, cmd, position)?
        }
        Command::Result(crate::commands::ResultOp { .. }) => apply_result(w, actor, cmd)?,
        Command::Finish(crate::commands::FinishOp { .. }) => apply_finish(w, actor, cmd, position)?,
        Command::Accept(crate::commands::AcceptOp { .. }) => apply_accept(w, actor, cmd, position)?,
        Command::Revoke(crate::commands::RevokeOp {
            req,
            revision: n,
            reason,
        }) => {
            nonempty(reason)?;
            let r = revision(w, req, *n, true)?;
            ensure!(r.owner == actor, "only owner revokes acceptance");
            ensure!(r.acceptance.is_some(), "no acceptance to revoke");
            r.acceptance = None;
        }
        Command::Issue(crate::commands::IssueOp { .. }) => apply_issue(w, actor, cmd, position)?,
        Command::Resolve(crate::commands::ResolveOp { .. }) => {
            apply_resolve(w, actor, cmd, position)?
        }
        _ => crate::intake::apply(w, actor, cmd)?,
    }
    Ok(())
}

pub(crate) fn check_comment(w: &Workspace, req: &str, n: u32, id: &str) -> Result<()> {
    ensure!(
        w.requirements
            .get(req)
            .and_then(|q| q.comments.get(id))
            .is_some_and(|c| c.revision == n),
        "link a comment on this revision"
    );
    Ok(())
}
