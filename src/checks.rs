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
    let accepted = green
        && r.acceptance
            .as_ref()
            .is_some_and(|a| run.is_some_and(|(id, _)| id == &a.run));
    json!({"revision": r.number, "acs": statuses, "run": run.map(|(id, _)| id), "ready_for_acceptance": green, "accepted": accepted})
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
        } => {
            identifier(id)?;
            nonempty(title)?;
            ensure!(w.members.contains_key(assignee), "unknown assignee");
            ensure!(
                ["implementation", "decision", "manual"].contains(&purpose.as_str()),
                "unknown task purpose"
            );
            for project in projects {
                ensure!(w.projects.contains_key(project), "unknown project");
            }
            let r = revision(w, req, *n, true)?;
            ensure!(!r.tasks.contains_key(id), "task exists");
            ensure!(!acs.is_empty(), "task needs at least one AC");
            for ac in acs {
                ensure!(r.acs.iter().any(|a| &a.id == ac), "unknown AC");
                ensure!(
                    purpose == "decision" || ready(r, ac),
                    "AC awaits decision or dependent AC"
                );
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
                },
            );
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
                task.assignee == actor || r.owner == actor,
                "assignee or owner required"
            );
            ensure!(task.done.is_none(), "task already completed");
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
                        a.decider.as_ref().is_none_or(|d| d == &decision.actor),
                        "designated specialist must decide"
                    );
                }
            } else {
                ensure!(task.acs.iter().all(|a| ready(r, a)), "AC no longer ready");
            }
            r.tasks.get_mut(id).unwrap().done = Some(evidence.clone());
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
            let r = revision(w, req, *n, true)?;
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
