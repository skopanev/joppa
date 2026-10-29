//! Arms of `apply` in checks.rs, one function for each operation.
use super::*;

/// Apply `complete_task`.
pub(super) fn apply_complete_task(w: &mut Workspace, actor: &str, cmd: &Command) -> Result<()> {
    let Command::CompleteTask(crate::commands::CompleteTaskOp {
        req,
        revision: n,
        id,
        evidence,
        ..
    }) = cmd
    else {
        unreachable!()
    };
    nonempty(evidence)?;
    let decisions = w
        .requirements
        .get(req)
        .map(|q| q.comments.clone())
        .unwrap_or_default();
    let r = revision(w, req, *n, true)?;
    let task = r
        .tasks
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("unknown task"))?;
    ensure!(
        task.assignee.as_deref() == Some(actor) || r.owner == actor,
        "assignee or owner required"
    );
    ensure!(task.assignee.is_some(), "claim task before completing it");
    ensure!(task.done.is_none(), "task already completed");
    ensure!(
        !task.review_required,
        "task needs owner review after REQ revision"
    );
    ensure!(task.retired.is_none(), "task retired after REQ revision");
    if task.purpose == "decision" {
        ensure!(
            task.acs.len() == 1,
            "one decision task covers one AC; use separate tasks for separate decisions"
        );
        let decision = decisions
            .get(evidence)
            .ok_or_else(|| anyhow::anyhow!("decision task evidence must be a comment id"))?;
        ensure!(
            decision.kind == "decision"
                && !decision.withdrawn
                && decision.revision == *n
                && decision.ac.as_ref().is_some_and(|a| task.acs.contains(a)),
            "link decision for this revision and AC"
        );
        for ac in &task.acs {
            let a = r.acs.iter().find(|a| &a.id == ac).unwrap();
            ensure!(
                a.owner.as_ref().is_none_or(|d| d == &decision.actor),
                "designated specialist must decide"
            );
        }
    } else {
        ensure!(task.acs.iter().all(|a| ready(r, a)), "AC no longer ready");
    }
    r.tasks.get_mut(id).unwrap().done = Some(evidence.clone());
    Ok(())
}

/// Apply `define_verification_scope`.
pub(super) fn apply_define_verification_scope(
    w: &mut Workspace,
    actor: &str,
    cmd: &Command,
    position: u64,
) -> Result<()> {
    let Command::DefineVerificationScope(crate::commands::DefineVerificationScopeOp {
        req,
        revision: n,
        subject_keys,
        scope,
        reason,
    }) = cmd
    else {
        unreachable!()
    };
    nonempty(scope)?;
    nonempty(reason)?;
    ensure!(
        !subject_keys.is_empty(),
        "verification scope needs at least one subject key"
    );
    let mut seen = BTreeSet::new();
    for key in subject_keys {
        nonempty(key)?;
        ensure!(seen.insert(key), "duplicate subject key");
    }
    let r = revision(w, req, *n, true)?;
    ensure!(r.owner == actor, "REQ owner defines verification scope");
    ensure!(
        r.runs.is_empty(),
        "verification scope is sealed by the first check plan; revise REQ to change it"
    );
    r.verification_scope = Some(VerificationScope {
        subject_keys: subject_keys.clone(),
        scope: scope.clone(),
        actor: actor.into(),
        at: position,
        reason: reason.clone(),
    });
    Ok(())
}
