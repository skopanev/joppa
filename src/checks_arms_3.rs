//! Arms of `apply` in checks.rs, one function for each operation.
use super::*;

/// Apply `plan`.
pub(super) fn apply_plan(
    w: &mut Workspace,
    actor: &str,
    cmd: &Command,
    position: u64,
) -> Result<()> {
    let Command::Plan(crate::commands::PlanOp {
        req,
        revision: n,
        run,
        subject,
        scope,
        discovery_complete,
        checks,
        enforce_coverage,
    }) = cmd
    else {
        unreachable!()
    };
    let normalized_checks: Vec<_> = checks
        .iter()
        .cloned()
        .map(|mut check| {
            for binding in &mut check.tasks {
                binding.address = crate::objects::resolve_address(w, &binding.address).to_owned();
            }
            check
        })
        .collect();
    let checks = &normalized_checks;
    validate_task_bindings(w, req, checks)?;
    let task_contexts = checks
        .iter()
        .flat_map(|check| &check.tasks)
        .map(|binding| {
            let address = crate::objects::resolve_address(w, &binding.address);
            (address.to_owned(), crate::tasks::context(w, address))
        })
        .collect();
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
    let old_checks: Vec<Check> = if let Some(current) = w
        .requirements
        .get(req)
        .and_then(|q| q.revisions.last())
        .filter(|r| !r.prior_checks.is_empty())
    {
        current.prior_checks.clone()
    } else if *n > 1 {
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
    let current_bindings: BTreeSet<_> = checks
        .iter()
        .flat_map(|check| &check.tasks)
        .map(|binding| (binding.address.clone(), binding.origin_revision))
        .collect();
    let obsolete_bindings: BTreeSet<_> = w
        .requirements
        .get(req)
        .and_then(|q| q.revisions.last())
        .into_iter()
        .flat_map(|r| r.runs.values())
        .flat_map(|run| &run.plan)
        .flat_map(|check| &check.tasks)
        .filter(|binding| {
            crate::tasks::task(w, &binding.address).is_none_or(|task| {
                task.retired.is_some() || task.origin_revision != binding.origin_revision
            })
        })
        .map(|binding| (binding.address.clone(), binding.origin_revision))
        .collect();
    let task_projects: Vec<String> = w
        .task_process
        .iter()
        .filter(|(key, t)| {
            t.retired.is_none()
                && crate::objects::within(w, key, &crate::objects::requirement_address(req))
        })
        .map(|(key, _)| crate::tasks::subjects(w, key))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .chain(
            w.requirements
                .get(req)
                .and_then(|q| q.revisions.last())
                .into_iter()
                .flat_map(|r| r.tasks.iter())
                .filter(|(id, t)| {
                    t.retired.is_none() && !w.task_process.contains_key(&format!("task:{req}/{id}"))
                })
                .flat_map(|(_, t)| t.projects.clone()),
        )
        .collect();
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
            task_projects.iter().all(|p| required.contains(p)),
            "subject omits a project used by a Task"
        );
        ensure!(
            actual == required,
            "subject must contain exactly the owner-approved component keys"
        );
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
                    && n.expected == c.expected
                    && c.tasks.iter().all(|binding| n.tasks.contains(binding)
                        || obsolete_bindings
                            .contains(&(binding.address.clone(), binding.origin_revision)))),
                "expected check disappeared/changed; revise REQ to replace its verification contract"
            );
        }
    }
    if r.coverage_policy || *enforce_coverage {
        let mut changes: Vec<Check> = old_checks
            .into_iter()
            .filter(|old| !checks.iter().any(|new| new == old))
            .collect();
        for check in r
            .coverage_change
            .iter()
            .chain(r.runs.values().flat_map(|run| &run.plan))
        {
            if check.tasks.iter().any(|binding| {
                obsolete_bindings.contains(&(binding.address.clone(), binding.origin_revision))
                    && !current_bindings
                        .contains(&(binding.address.clone(), binding.origin_revision))
            }) && !changes.contains(check)
            {
                changes.push(check.clone());
            }
        }
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
            task_contexts,
        },
    );
    Ok(())
}
