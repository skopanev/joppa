//! Arms of `apply` in checks.rs, one function for each operation.
use super::*;

/// Apply `review_coverage`.
pub(super) fn apply_review_coverage(
    w: &mut Workspace,
    actor: &str,
    cmd: &Command,
    position: u64,
) -> Result<()> {
    let Command::ReviewCoverage(crate::commands::ReviewCoverageOp {
        req,
        revision: n,
        run,
        reason,
    }) = cmd
    else {
        unreachable!()
    };
    nonempty(reason)?;
    let r = revision(w, req, *n, true)?;
    ensure!(r.owner == actor, "REQ owner reviews verification coverage");
    ensure!(
        !r.coverage_change.is_empty(),
        "no coverage change to review"
    );
    let latest = r
        .runs
        .iter()
        .max_by_key(|(_, item)| item.started)
        .map(|(id, _)| id);
    ensure!(latest == Some(run), "review the latest sealed check plan");
    r.coverage_review = Some(CoverageReview {
        actor: actor.into(),
        at: position,
        reason: reason.clone(),
        changes: r.coverage_change.clone(),
    });
    Ok(())
}

/// Apply `result`.
pub(super) fn apply_result(w: &mut Workspace, actor: &str, cmd: &Command) -> Result<()> {
    let Command::Result(crate::commands::ResultOp {
        req,
        revision: n,
        run,
        evidence,
    }) = cmd
    else {
        unreachable!()
    };
    ensure!(
        ["pass", "fail", "skip"].contains(&evidence.outcome.as_str()),
        "unknown outcome"
    );
    let is_human = w.members.get(actor).is_some_and(|r| r == "human");
    let r = revision(w, req, *n, false)?;
    let run = r
        .runs
        .get_mut(run)
        .ok_or_else(|| anyhow::anyhow!("unknown run"))?;
    ensure!(run.finished.is_none(), "run is finished");
    let check = run
        .plan
        .iter()
        .find(|c| c.id == evidence.check)
        .ok_or_else(|| anyhow::anyhow!("check not in sealed plan"))?;
    ensure!(
        if check.method == "manual" {
            is_human
        } else {
            run.reporter == actor
        },
        "manual evidence needs a human; automated evidence needs the run reporter"
    );
    ensure!(
        !run.results.contains_key(&evidence.check),
        "result is immutable; start another run"
    );
    if evidence.outcome != "skip" {
        ensure!(!evidence.observed.is_empty(), "observed evidence required");
        for e in &evidence.observed {
            nonempty(e)?;
        }
        nonempty(&evidence.note)?;
    }
    if check.method == "manual" && evidence.outcome == "pass" {
        let expected: BTreeSet<_> = check.expected.iter().collect();
        let actual: BTreeSet<_> = evidence.observed.iter().collect();
        ensure!(
            expected == actual,
            "observed list differs from expected; report fail"
        );
    }
    run.results.insert(evidence.check.clone(), evidence.clone());
    Ok(())
}

/// Apply `finish`.
pub(super) fn apply_finish(
    w: &mut Workspace,
    actor: &str,
    cmd: &Command,
    position: u64,
) -> Result<()> {
    let Command::Finish(crate::commands::FinishOp {
        req,
        revision: n,
        run,
    }) = cmd
    else {
        unreachable!()
    };
    let r = revision(w, req, *n, false)?;
    let run = r
        .runs
        .get_mut(run)
        .ok_or_else(|| anyhow::anyhow!("unknown run"))?;
    ensure!(run.reporter == actor, "only run reporter can finish");
    ensure!(run.finished.is_none(), "run finished");
    // Missing results remain n/a; finish never manufactures skipped or passing evidence.
    run.finished = Some(position);
    Ok(())
}

/// Apply `accept`.
pub(super) fn apply_accept(
    w: &mut Workspace,
    actor: &str,
    cmd: &Command,
    position: u64,
) -> Result<()> {
    let Command::Accept(crate::commands::AcceptOp {
        req,
        revision: n,
        run,
    }) = cmd
    else {
        unreachable!()
    };
    crate::commands::person(w, actor)?;
    let r = revision(w, req, *n, true)?;
    ensure!(r.owner == actor, "only REQ owner accepts");
    ensure!(
        r.coverage_change.is_empty()
            || r.coverage_review
                .as_ref()
                .is_some_and(|review| review.changes == r.coverage_change),
        "REQ owner must review checks removed or changed since the prior revision"
    );
    let (id, last) = latest(r).ok_or_else(|| anyhow::anyhow!("no completed run"))?;
    ensure!(id == run, "cannot select an older run");
    ensure!(
        !r.acs.is_empty() && r.acs.iter().all(|a| verdict(r, last, &a.id) == "pass"),
        "all ACs must be verified on the same subject"
    );
    r.acceptance = Some(Acceptance {
        run: run.clone(),
        actor: actor.into(),
        at: position,
    });
    Ok(())
}

/// Apply `issue`.
pub(super) fn apply_issue(
    w: &mut Workspace,
    actor: &str,
    cmd: &Command,
    position: u64,
) -> Result<()> {
    let Command::Issue(crate::commands::IssueOp {
        req,
        revision: n,
        id,
        ac,
        comment,
    }) = cmd
    else {
        unreachable!()
    };
    identifier(id)?;
    check_comment(w, req, *n, comment)?;
    let r = revision(w, req, *n, true)?;
    ensure!(
        r.owner == actor,
        "owner confirms reported defect; others leave comments"
    );
    ensure!(r.acs.iter().any(|a| &a.id == ac), "unknown AC");
    ensure!(!r.issues.contains_key(id), "issue exists");
    r.issues.insert(
        id.clone(),
        Issue {
            ac: ac.clone(),
            comment: comment.clone(),
            opened: position,
            resolved: None,
        },
    );
    r.acceptance = None;
    Ok(())
}

/// Apply `resolve`.
pub(super) fn apply_resolve(
    w: &mut Workspace,
    actor: &str,
    cmd: &Command,
    position: u64,
) -> Result<()> {
    let Command::Resolve(crate::commands::ResolveOp {
        req,
        revision: n,
        id,
        comment,
    }) = cmd
    else {
        unreachable!()
    };
    check_comment(w, req, *n, comment)?;
    let r = revision(w, req, *n, true)?;
    ensure!(r.owner == actor, "owner confirms resolution");
    let issue = r
        .issues
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("unknown issue"))?;
    ensure!(issue.resolved.is_none(), "issue already resolved");
    issue.resolved = Some(position);
    Ok(())
}
