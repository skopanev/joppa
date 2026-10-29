//! Part of `checks`: `latest` and the items after it, moved from checks.rs unchanged.
use super::*;

pub fn latest(r: &Revision) -> Option<(&String, &Run)> {
    r.runs
        .iter()
        .filter(|(_, run)| run.finished.is_some())
        .max_by_key(|(_, run)| run.started)
}

pub(crate) fn validate_task_bindings(w: &Workspace, req: &str, checks: &[Check]) -> Result<()> {
    for check in checks {
        let mut seen = BTreeSet::new();
        for binding in &check.tasks {
            let key = crate::objects::resolve_address(w, &binding.address);
            ensure!(seen.insert(key), "duplicate Task binding in a check");
            let obj = w
                .objects
                .get(key)
                .ok_or_else(|| anyhow::anyhow!("unknown bound Task"))?;
            ensure!(obj.current().present, "bound Task is absent");
            ensure!(
                crate::objects::schema(w)
                    .types
                    .get(&obj.kind)
                    .is_some_and(|kind| kind.role == "task"),
                "binding requires a configured Task role"
            );
            let task = crate::tasks::task(w, key)
                .ok_or_else(|| anyhow::anyhow!("unknown bound Task process"))?;
            ensure!(task.retired.is_none(), "bound Task is retired");
            ensure!(
                task.origin_revision == binding.origin_revision,
                "stale Task contract revision"
            );
            ensure!(
                !crate::tasks::review_required(w, key, task),
                "bound Task needs source review"
            );
            ensure!(
                crate::objects::verification_context(w, key)
                    .is_some_and(|(context, _)| context == req),
                "bound Task belongs to another verification context"
            );
            let mut parent = obj.current().parent.as_deref();
            let mut parents = BTreeSet::new();
            while let Some(address) = parent {
                let address = crate::objects::resolve_address(w, address);
                ensure!(parents.insert(address), "Task parent cycle");
                if let Some((_, _, criterion)) = crate::objects::criterion_context(w, address) {
                    ensure!(
                        criterion.id == check.ac,
                        "bound Task belongs to another criterion"
                    );
                    break;
                }
                let source = w
                    .objects
                    .get(address)
                    .ok_or_else(|| anyhow::anyhow!("unknown Task parent"))?;
                if crate::objects::schema(w)
                    .types
                    .get(&source.kind)
                    .is_some_and(|kind| kind.role == "requirement")
                {
                    break;
                }
                parent = source.current().parent.as_deref();
            }
        }
    }
    Ok(())
}

pub fn task_verdict(w: &Workspace, key: &str) -> &'static str {
    let key = crate::objects::resolve_address(w, key);
    let Some(task) = crate::tasks::task(w, key) else {
        return "unverified";
    };
    if task.retired.is_some()
        || crate::tasks::review_required(w, key, task)
        || crate::tasks::readiness_blocker(w, key).is_some()
    {
        return "unverified";
    }
    // Work under a signal has no criterion to verify.
    if w.objects
        .get(key)
        .and_then(|o| o.current().parent.as_deref())
        .and_then(|p| w.objects.get(crate::objects::resolve_address(w, p)))
        .is_some_and(|p| crate::objects::input_type(w, &p.kind))
    {
        return "none";
    }
    let Some((_, revision)) = crate::objects::verification_context(w, key) else {
        return "unverified";
    };
    let Some(run) = revision.runs.values().max_by_key(|run| run.started) else {
        return "unverified";
    };
    if run.finished.is_none() || !run.discovery_complete {
        return "unverified";
    }
    if run.task_contexts.get(key) != Some(&crate::tasks::context(w, key)) {
        return "unverified";
    }
    let checks: Vec<_> = run
        .plan
        .iter()
        .filter(|check| {
            check.tasks.iter().any(|binding| {
                crate::objects::resolve_address(w, &binding.address) == key
                    && binding.origin_revision == task.origin_revision
            })
        })
        .collect();
    if checks.is_empty() {
        return "unverified";
    }
    if checks.iter().any(|check| {
        run.results
            .get(&check.id)
            .is_some_and(|result| result.outcome == "fail")
    }) {
        return "fail";
    }
    if checks.iter().any(|check| {
        run.results
            .get(&check.id)
            .is_none_or(|result| result.outcome != "pass")
    }) {
        return "unverified";
    }
    if !revision.coverage_change.is_empty()
        && revision
            .coverage_review
            .as_ref()
            .is_none_or(|review| review.changes != revision.coverage_change)
    {
        return "unverified";
    }
    "pass"
}

pub fn verdict(r: &Revision, run: &Run, ac: &str) -> &'static str {
    if !r.confirmed || !ready(r, ac) || !run.discovery_complete || run.finished.is_none() {
        return "n/a";
    }
    if r.issues
        .values()
        .any(|i| i.ac == ac && (i.resolved.is_none() || i.resolved.unwrap() >= run.started))
    {
        return "n/a";
    }
    let plan: Vec<_> = run.plan.iter().filter(|c| c.ac == ac).collect();
    if plan.is_empty() {
        return "n/a";
    }
    if plan
        .iter()
        .any(|c| run.results.get(&c.id).is_some_and(|e| e.outcome == "fail"))
    {
        return "fail";
    }
    if plan
        .iter()
        .any(|c| run.results.get(&c.id).is_none_or(|e| e.outcome != "pass"))
    {
        return "n/a";
    }
    "pass"
}

pub fn summary(r: &Revision) -> Value {
    let run = latest(r);
    let pending_run = r
        .runs
        .iter()
        .filter(|(_, candidate)| {
            candidate.finished.is_none()
                && run.is_none_or(|(_, completed)| candidate.started > completed.started)
        })
        .max_by_key(|(_, candidate)| candidate.started)
        .map(|(id, _)| id);
    let statuses: BTreeMap<_, _> = r.acs.iter().map(|a| (a.id.clone(), json!({"ready": ready(r, &a.id), "verification": run.map_or("n/a", |(_, run)| verdict(r, run, &a.id))}))).collect();
    let green = !r.acs.is_empty()
        && run.is_some_and(|(_, run)| r.acs.iter().all(|a| verdict(r, run, &a.id) == "pass"));
    let coverage_reviewed = r.coverage_change.is_empty()
        || r.coverage_review
            .as_ref()
            .is_some_and(|review| review.changes == r.coverage_change);
    let accepted = green
        && coverage_reviewed
        && r.acceptance
            .as_ref()
            .is_some_and(|a| run.is_some_and(|(id, _)| id == &a.run));
    json!({"revision": r.number, "acs": statuses, "run": run.map(|(id, _)| id), "pending_run": pending_run, "ready_for_acceptance": green && coverage_reviewed, "accepted": accepted,
        "coverage_change": r.coverage_change, "coverage_reviewed": coverage_reviewed})
}

/// The same predicate powers explicit claims, claim-next, and the MCP queue.
pub fn claimable(
    w: &Workspace,
    actor: &str,
    req: &str,
    id: &str,
    r: &Revision,
    task: &Task,
) -> Result<()> {
    validate_claim(w, actor, req, id, r, task)?;
    crate::tasks::ensure_agent_available(w, actor, &crate::tasks::address(req, id))
}
