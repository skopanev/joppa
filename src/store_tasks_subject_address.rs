//! Part of `store_tasks`: `subject_address` and the items after it, moved from store_tasks.rs unchanged.
use super::*;

/// A project selector is an ordinary object address once the schema owns
/// relations. Its membership comes from the configured verification subject,
/// declared on the Task itself or on any of its ancestors.
pub(crate) fn subject_address(state: &Workspace, selector: &str) -> Option<String> {
    if selector.contains(':') {
        let address = crate::objects::resolve_address(state, selector);
        return state
            .objects
            .get(address)
            .filter(|o| o.current().present)
            .map(|_| address.to_owned());
    }
    let schema = crate::objects::schema(state);
    let mut candidates = state.objects.iter().filter(|(_, o)| {
        o.current().present
            && o.id == selector
            && schema.types.values().any(|t| {
                t.references
                    .values()
                    .any(|r| r.verification_subject && (r.target == "*" || r.target == o.kind))
            })
    });
    let first = candidates.next()?.0.clone();
    candidates.next().is_none().then_some(first)
}

pub(crate) fn priority_address(state: &Workspace, selector: &str) -> Option<String> {
    let resolved = crate::objects::resolve_address(state, selector);
    if state
        .objects
        .get(resolved)
        .is_some_and(|o| o.current().present)
    {
        return Some(resolved.into());
    }
    let (kind, id) = selector.split_once(':')?;
    let mut matches = state.objects.iter().filter(|(_, o)| {
        o.current().present && o.kind == kind && o.id.rsplit('/').next() == Some(id)
    });
    let first = matches.next()?.0.clone();
    matches.next().is_none().then_some(first)
}

pub(crate) fn task_in_project(
    state: &Workspace,
    key: &str,
    t: &crate::model::Task,
    project: &str,
) -> bool {
    if state.schema_migrated {
        subject_address(state, project).is_some_and(|target| subject_of(state, key, &target))
    } else {
        t.projects.iter().any(|id| id == project)
    }
}

pub(crate) fn project_target(state: &Workspace, project: &str) -> bool {
    subject_address(state, project).is_some()
}

pub(crate) fn subject_of(state: &Workspace, key: &str, target: &str) -> bool {
    let mut current = Some(key.to_owned());
    let mut seen = std::collections::BTreeSet::new();
    while let Some(address) = current {
        if !seen.insert(address.clone()) {
            return false;
        }
        let Some(o) = state.objects.get(&address) else {
            return false;
        };
        if crate::objects::stored_reference_targets(state, &o.kind, &o.current().data).is_ok_and(
            |refs| {
                refs.iter()
                    .any(|(_, resolved, subject)| *subject && resolved == target)
            },
        ) {
            return true;
        }
        current = o.current().parent.clone();
    }
    false
}

pub(crate) fn validate_selection<'a>(
    state: &Workspace,
    project: Option<&str>,
    purpose: Option<&str>,
    priorities: &'a [String],
) -> Result<Vec<(&'a str, &'a str)>> {
    if let Some(project) = project {
        ensure!(
            if state.schema_migrated {
                project_target(state, project)
            } else {
                state.projects.contains_key(project)
            },
            "unknown project"
        );
    }
    if let Some(purpose) = purpose {
        ensure!(!purpose.trim().is_empty(), "task purpose must be nonempty");
    }
    priorities
        .iter()
        .map(|selector| {
            let (kind, id) = selector
                .split_once(':')
                .context("priority must be kind:id")?;
            ensure!(!id.is_empty(), "priority ID is empty");
            if state.schema_migrated {
                ensure!(
                    priority_address(state, selector).is_some(),
                    "unknown priority target: {selector}"
                );
                return Ok((kind, id));
            }
            let exists = match kind {
                "domain" => state.domains.contains_key(id),
                "capability" => state.capabilities.contains_key(id),
                "requirement" => state.requirements.contains_key(id),
                "ac" => state.requirements.values().any(|q| {
                    q.revisions
                        .last()
                        .is_some_and(|r| r.acs.iter().any(|ac| ac.id == id))
                }),
                "task" => {
                    state
                        .objects
                        .get(selector)
                        .is_some_and(|o| o.current().present)
                        || state.task_process.keys().any(|key| {
                            key.strip_prefix("task:")
                                .is_some_and(|key| key.rsplit('/').next() == Some(id))
                        })
                        || state
                            .requirements
                            .values()
                            .any(|q| q.revisions.last().is_some_and(|r| r.tasks.contains_key(id)))
                }
                "project" => state.projects.contains_key(id),
                "consumer" => state.consumers.contains_key(id),
                _ => state
                    .objects
                    .get(selector)
                    .is_some_and(|o| o.current().present),
            };
            ensure!(exists, "unknown priority target: {selector}");
            Ok((kind, id))
        })
        .collect()
}

pub(crate) fn rank(
    state: &Workspace,
    req: &str,
    q: &crate::model::Requirement,
    revision: &crate::model::Revision,
    id: &str,
    task: &crate::model::Task,
    priorities: &[(&str, &str)],
) -> usize {
    priorities
        .iter()
        .position(|(kind, target)| {
            crate::objects::within(
                state,
                &format!("task:{req}/{id}"),
                &format!("{kind}:{target}"),
            ) || match *kind {
                "domain" => q
                    .capability
                    .as_ref()
                    .and_then(|id| state.capabilities.get(id))
                    .is_some_and(|c| c.domain.as_deref() == Some(target)),
                "capability" => q.capability.as_deref() == Some(target),
                "requirement" => req == *target,
                "ac" => task.acs.iter().any(|ac| ac == target),
                "task" => id == *target || format!("{req}/{id}") == *target,
                "project" => task.projects.iter().any(|project| project == target),
                "consumer" => revision.consumers.iter().any(|consumer| consumer == target),
                _ => false,
            }
        })
        .unwrap_or(priorities.len())
}

pub(crate) fn object_rank(
    state: &Workspace,
    key: &str,
    t: &crate::model::Task,
    priorities: &[(&str, &str)],
) -> usize {
    priorities
        .iter()
        .position(|(kind, id)| {
            let selector = format!("{kind}:{id}");
            let target = priority_address(state, &selector).unwrap_or(selector);
            crate::objects::within(state, key, &target)
                || crate::objects::referenced(state, key, &target)
                || !state.schema_migrated
                    && (*kind == "project" && t.projects.iter().any(|p| p == id)
                        || *kind == "task"
                            && (key == format!("task:{id}") || key.rsplit('/').next() == Some(id)))
        })
        .unwrap_or(priorities.len())
}
