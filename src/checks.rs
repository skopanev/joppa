use crate::{
    commands::{Command, identifier, nonempty, ready, revision},
    model::*,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub fn latest(r: &Revision) -> Option<(&String, &Run)> {
    r.runs
        .iter()
        .filter(|(_, run)| run.finished.is_some())
        .max_by_key(|(_, run)| run.started)
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
    json!({"revision": r.number, "acs": statuses, "run": run.map(|(id, _)| id), "ready_for_acceptance": green && coverage_reviewed, "accepted": accepted,
        "coverage_change": r.coverage_change, "coverage_reviewed": coverage_reviewed})
}

/// The same predicate powers explicit claims, claim-next, and the MCP queue.
pub fn claimable(w: &Workspace, actor: &str, r: &Revision, task: &Task) -> Result<()> {
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
        Command::Task {
            req,
            revision: n,
            id,
            title,
            acs,
            projects,
            purpose,
            assignee,
            contract,
        } => {
            identifier(id)?;
            nonempty(title)?;
            if let Some(assignee) = assignee {
                ensure!(w.members.contains_key(assignee), "unknown assignee");
            } else {
                nonempty(contract)?;
            }
            ensure!(
                ["implementation", "decision", "manual"].contains(&purpose.as_str()),
                "unknown task purpose"
            );
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
                    title: title.clone(),
                    acs: acs.clone(),
                    projects: projects.clone(),
                    purpose: purpose.clone(),
                    assignee: assignee.clone(),
                    done: None,
                    contract: contract.clone(),
                    created_at: position,
                    created_by: actor.into(),
                    review_required: false,
                    origin_revision: *n,
                    retired: None,
                },
            );
        }
        Command::ClaimTask {
            req,
            revision: n,
            id,
        } => {
            let q = w
                .requirements
                .get(req)
                .ok_or_else(|| anyhow::anyhow!("unknown REQ"))?;
            ensure!(
                q.revisions.len() == *n as usize && *n > 0,
                "stale revision; read the task again"
            );
            let r = q.revisions.last().unwrap();
            let task = r
                .tasks
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("unknown task"))?;
            claimable(w, actor, r, task)?;
            revision(w, req, *n, true)?
                .tasks
                .get_mut(id)
                .unwrap()
                .assignee = Some(actor.into());
        }
        Command::AmendTask {
            req,
            revision: n,
            id,
            title,
            contract,
            projects,
            reason,
        } => {
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
        }
        Command::ReleaseTask {
            req,
            revision: n,
            id,
            reason,
        } => {
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
        }
        Command::ReviewTask {
            req,
            revision: n,
            id,
            ac,
            reason,
        } => {
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
        }
        Command::RetireTask {
            req,
            revision: n,
            id,
            reason,
        } => {
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
        }
        Command::CompleteTask {
            req,
            revision: n,
            id,
            evidence,
        } => {
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
                let decision = decisions.get(evidence).ok_or_else(|| {
                    anyhow::anyhow!("decision task evidence must be a comment id")
                })?;
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
        }
        Command::DefineVerificationScope {
            req,
            revision: n,
            subject_keys,
            scope,
            reason,
        } => {
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
        }
        Command::Plan {
            req,
            revision: n,
            run,
            subject,
            scope,
            discovery_complete,
            checks,
        } => {
            identifier(run)?;
            nonempty(scope)?;
            ensure!(
                !subject.is_empty(),
                "subject must identify builds/configuration/environment"
            );
            for (k, v) in subject {
                nonempty(k)?;
                nonempty(v)?;
            }
            let old_checks: Vec<Check> = if *n > 1 {
                w.requirements
                    .get(req)
                    .and_then(|q| {
                        q.revisions
                            .iter()
                            .take((*n - 1) as usize)
                            .rev()
                            .find_map(|old| old.runs.values().max_by_key(|run| run.started))
                    })
                    .map(|run| run.plan.clone())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let r = revision(w, req, *n, true)?;
            if r.coverage_policy {
                let contract = r.verification_scope.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("REQ owner must define verification scope before a check plan")
                })?;
                ensure!(
                    scope.trim() == contract.scope.trim(),
                    "check scope differs from the owner-approved scope"
                );
                let actual: BTreeSet<_> = subject.keys().collect();
                let required: BTreeSet<_> = contract.subject_keys.iter().collect();
                ensure!(
                    actual == required,
                    "subject must contain exactly the owner-approved component keys"
                );
                for task in r.tasks.values().filter(|task| task.retired.is_none()) {
                    for project in &task.projects {
                        ensure!(
                            required.contains(project),
                            "subject omits a project used by a task"
                        );
                    }
                }
                for (component, version) in subject {
                    ensure!(
                        component == "environment"
                            || ![
                                "dev",
                                "development",
                                "test",
                                "staging",
                                "stage",
                                "production",
                                "prod"
                            ]
                            .contains(&version.to_ascii_lowercase().as_str()),
                        "component version must identify a build or configuration, not an environment name"
                    );
                }
            }
            ensure!(!r.runs.contains_key(run), "run exists; plan is sealed");
            ensure!(!checks.is_empty(), "empty plan");
            let mut ids = BTreeSet::new();
            for c in checks {
                nonempty(&c.id)?;
                ensure!(ids.insert(&c.id), "one test maps to exactly one AC");
                ensure!(r.acs.iter().any(|a| a.id == c.ac), "unknown AC");
                ensure!(
                    ["automated", "manual"].contains(&c.method.as_str()),
                    "unknown check method"
                );
                ensure!(!c.expected.is_empty(), "expected evidence is required");
                for e in &c.expected {
                    nonempty(e)?;
                }
                if c.method == "automated" {
                    let tags: Vec<_> =
                        c.id.split('[')
                            .skip(1)
                            .filter_map(|s| s.split_once(']').map(|(tag, _)| tag))
                            .collect();
                    ensure!(
                        tags == vec![c.ac.as_str()],
                        "automated test id must contain exactly one [AC-id]"
                    );
                }
            }
            for a in &r.acs {
                ensure!(
                    checks.iter().any(|c| c.ac == a.id),
                    "full plan must cover every AC, including waiting ACs"
                );
            }
            for old in r.runs.values() {
                for c in &old.plan {
                    ensure!(
                        checks.iter().any(|n| n.id == c.id
                            && n.ac == c.ac
                            && n.method == c.method
                            && n.expected == c.expected),
                        "expected check disappeared/changed; revise REQ to replace its verification contract"
                    );
                }
            }
            if r.coverage_policy {
                let changes: Vec<Check> = old_checks
                    .into_iter()
                    .filter(|old| !checks.iter().any(|new| new == old))
                    .collect();
                if r.coverage_review
                    .as_ref()
                    .is_some_and(|review| review.changes != changes)
                {
                    r.coverage_review = None;
                }
                r.coverage_change = changes;
            }
            r.runs.insert(
                run.clone(),
                Run {
                    subject: subject.clone(),
                    plan: checks.clone(),
                    discovery_complete: *discovery_complete,
                    scope: scope.clone(),
                    reporter: actor.into(),
                    started: position,
                    finished: None,
                    results: BTreeMap::new(),
                },
            );
        }
        Command::ReviewCoverage {
            req,
            revision: n,
            run,
            reason,
        } => {
            nonempty(reason)?;
            let r = revision(w, req, *n, true)?;
            ensure!(r.owner == actor, "REQ owner reviews verification coverage");
            ensure!(
                r.coverage_policy && !r.coverage_change.is_empty(),
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
        }
        Command::Result {
            req,
            revision: n,
            run,
            evidence,
        } => {
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
        }
        Command::Finish {
            req,
            revision: n,
            run,
        } => {
            let r = revision(w, req, *n, false)?;
            let run = r
                .runs
                .get_mut(run)
                .ok_or_else(|| anyhow::anyhow!("unknown run"))?;
            ensure!(run.reporter == actor, "only run reporter can finish");
            ensure!(run.finished.is_none(), "run finished");
            // Missing results remain n/a; finish never manufactures skipped or passing evidence.
            run.finished = Some(position);
        }
        Command::Accept {
            req,
            revision: n,
            run,
        } => {
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
        }
        Command::Revoke {
            req,
            revision: n,
            reason,
        } => {
            nonempty(reason)?;
            let r = revision(w, req, *n, true)?;
            ensure!(r.owner == actor, "only owner revokes acceptance");
            ensure!(r.acceptance.is_some(), "no acceptance to revoke");
            r.acceptance = None;
        }
        Command::Issue {
            req,
            revision: n,
            id,
            ac,
            comment,
        } => {
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
        }
        Command::Resolve {
            req,
            revision: n,
            id,
            comment,
        } => {
            check_comment(w, req, *n, comment)?;
            let r = revision(w, req, *n, true)?;
            ensure!(r.owner == actor, "owner confirms resolution");
            let issue = r
                .issues
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("unknown issue"))?;
            ensure!(issue.resolved.is_none(), "issue already resolved");
            issue.resolved = Some(position);
        }
        _ => crate::intake::apply(w, actor, cmd)?,
    }
    Ok(())
}
fn check_comment(w: &Workspace, req: &str, n: u32, id: &str) -> Result<()> {
    ensure!(
        w.requirements
            .get(req)
            .and_then(|q| q.comments.get(id))
            .is_some_and(|c| c.revision == n),
        "link a comment on this revision"
    );
    Ok(())
}
