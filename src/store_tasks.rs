use super::*;

/// A project selector is an ordinary object address once the schema owns
/// relations. Its membership comes from the configured verification subject,
/// declared on the Task itself or on any of its ancestors.
fn subject_address(state: &Workspace, selector: &str) -> Option<String> {
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
fn priority_address(state: &Workspace, selector: &str) -> Option<String> {
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
fn task_in_project(state: &Workspace, key: &str, t: &crate::model::Task, project: &str) -> bool {
    if state.schema_migrated {
        subject_address(state, project).is_some_and(|target| subject_of(state, key, &target))
    } else {
        t.projects.iter().any(|id| id == project)
    }
}
fn project_target(state: &Workspace, project: &str) -> bool {
    subject_address(state, project).is_some()
}
fn subject_of(state: &Workspace, key: &str, target: &str) -> bool {
    let mut current = Some(key.to_owned());
    let mut seen = std::collections::BTreeSet::new();
    while let Some(address) = current {
        if !seen.insert(address.clone()) {
            return false;
        }
        let Some(o) = state.objects.get(&address) else {
            return false;
        };
        if crate::objects::reference_targets(state, &o.kind, &o.current().data).is_ok_and(|refs| {
            refs.iter()
                .any(|(_, resolved, subject)| *subject && resolved == target)
        }) {
            return true;
        }
        current = o.current().parent.clone();
    }
    false
}

pub(super) fn validate_selection<'a>(
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

fn rank(
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

fn object_rank(
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

pub(super) fn select_next(
    state: &Workspace,
    actor: &str,
    project: Option<&str>,
    purpose: Option<&str>,
    priorities: &[String],
) -> Result<Option<(String, u32, String)>> {
    let priorities = validate_selection(state, project, purpose, priorities)?;
    let mut choices: Vec<(usize, u64, String, String, u32)> = state
        .requirements
        .iter()
        .filter_map(|(req, q)| q.revisions.last().map(|r| (req, q, r)))
        .flat_map(|(req, q, r)| r.tasks.iter().map(move |(id, t)| (req, q, r, id, t)))
        .filter(|(req, _, _, id, _)| !state.task_process.contains_key(&format!("task:{req}/{id}")))
        .filter(|(_, _, _, _, t)| {
            project.is_none_or(|p| t.projects.iter().any(|id| id == p))
                && purpose.is_none_or(|p| t.purpose == p)
        })
        .filter(|(req, _, r, id, t)| checks::claimable(state, actor, req, id, r, t).is_ok())
        .map(|(req, q, r, id, t)| {
            (
                rank(state, req, q, r, id, t, &priorities),
                t.created_at,
                req.clone(),
                id.clone(),
                r.number,
            )
        })
        .collect();
    let mut blocked = false;
    for (key, t) in &state.task_process {
        if project.is_some_and(|p| !task_in_project(state, key, t, p))
            || purpose.is_some_and(|p| t.purpose != p)
        {
            continue;
        }
        if let Err(error) = crate::tasks::claimable(state, actor, key, t) {
            blocked |= error.to_string() == crate::tasks::BLOCKED;
            continue;
        }
        choices.push((
            object_rank(state, key, t, &priorities),
            t.created_at,
            String::new(),
            key.clone(),
            state.objects[key].current().revision,
        ));
    }
    ensure!(!choices.is_empty() || !blocked, "{}", crate::tasks::BLOCKED);
    choices.sort_by(|a, b| (&a.0, &a.1, &a.2, &a.3).cmp(&(&b.0, &b.1, &b.2, &b.3)));
    Ok(choices
        .into_iter()
        .next()
        .map(|(_, _, req, id, revision)| (req, revision, id)))
}

/// Current Task queue, sourced from just the latest revision of each REQ in the
/// board projection. Neither old revisions nor the journal cross this read path.
pub async fn task_queue(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    view: &str,
    limit: usize,
    cursor: Option<&str>,
) -> Result<Value> {
    task_queue_filtered(pool, workspace, actor, view, limit, cursor, None, None, &[]).await
}

#[allow(clippy::too_many_arguments)]
pub async fn task_queue_filtered(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    view: &str,
    limit: usize,
    cursor: Option<&str>,
    project: Option<&str>,
    purpose: Option<&str>,
    priorities: &[String],
) -> Result<Value> {
    ensure!(
        ["available", "mine", "waiting", "all"].contains(&view),
        "unknown task queue view"
    );
    ensure!(
        (1..=100).contains(&limit),
        "limit must be between 1 and 100"
    );
    let mut client = pool.get().await?;
    let conn = client
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let workspace = resolve_workspace(&*conn, workspace).await?;
    access::check(&*conn, &workspace, actor, false).await?;
    super::queue_rows::read(
        &*conn, &workspace, actor, view, limit, cursor, project, purpose, priorities,
    )
    .await
}

type QueueEntry = (usize, u64, String, String, Value);

pub(super) fn queue_entries(
    state: &Workspace,
    actor: &str,
    selected: Option<&std::collections::BTreeSet<String>>,
) -> Result<Vec<QueueEntry>> {
    let project: Option<&str> = None;
    let purpose: Option<&str> = None;
    let priorities = vec![];
    let view = "all";
    let mut entries = vec![];
    for (req, requirement) in &state.requirements {
        let revision = requirement.revisions.last().unwrap();
        for (id, task) in &revision.tasks {
            if selected.is_some_and(|s| !s.contains(&format!("task:{req}/{id}"))) {
                continue;
            }
            if state.task_process.contains_key(&format!("task:{req}/{id}")) {
                continue;
            }
            if !project.is_none_or(|p| task.projects.iter().any(|id| id == p))
                || purpose.is_some_and(|p| task.purpose != p)
            {
                continue;
            }
            let claim_error = checks::validate_claim(state, actor, req, id, revision, task)
                .err()
                .map(|e| e.to_string());
            let status = if task.retired.is_some() {
                "retired"
            } else if task.done.is_some() {
                "done"
            } else if task.assignee.is_some() {
                "in_progress"
            } else {
                "open"
            };
            let include = match view {
                "available" => claim_error.is_none(),
                "mine" => {
                    task.assignee.as_deref() == Some(actor)
                        && task.done.is_none()
                        && task.retired.is_none()
                }
                "waiting" => status == "open" && claim_error.is_some(),
                _ => true,
            };
            if !include {
                continue;
            }
            let ac = task
                .acs
                .first()
                .and_then(|id| revision.acs.iter().find(|a| &a.id == id));
            entries.push((
                rank(state, req, requirement, revision, id, task, &priorities),
                task.created_at,
                req.clone(),
                id.clone(),
                json!({
                    "id":id,"req":req,"revision":revision.number,"ac":task.acs.first(),
                    "ac_text":ac.map(|a| &a.text),"title":task.title,"contract":task.contract,
                    "purpose":task.purpose,"projects":task.projects,"assignee":task.assignee,
                    "status":status,"claimable":claim_error.is_none(),"waiting_reason":claim_error,
                    "confirmed":revision.confirmed,"created_at":task.created_at
                }),
            ));
        }
    }
    for (key, t) in &state.task_process {
        if selected.is_some_and(|s| !s.contains(key)) {
            continue;
        }

        if project.is_some_and(|p| !task_in_project(state, key, t, p))
            || purpose.is_some_and(|p| t.purpose != p)
        {
            continue;
        }
        let claim_error = crate::tasks::validate_claim(state, actor, key, t)
            .err()
            .map(|e| e.to_string());
        let unclaimed_error = crate::tasks::unclaimed_eligibility(state, actor, key, t)
            .err()
            .map(|error| error.to_string());
        let status = crate::tasks::status(t);
        let include = match view {
            "available" => claim_error.is_none(),
            "mine" => {
                t.assignee.as_deref() == Some(actor) && t.done.is_none() && t.retired.is_none()
            }
            "waiting" => status == "open" && claim_error.is_some(),
            _ => true,
        };
        if !include {
            continue;
        }
        let o = &state.objects[key];
        entries.push((object_rank(state,key,t,&priorities),t.created_at,String::new(),key.clone(),json!({
            "id":key,"address":key,"req":null,"revision":o.current().revision,"parent":o.current().parent,
            "ac":o.current().parent.as_deref().and_then(|p|crate::objects::criterion_context(state,p).map(|(_,_,ac)|ac.id.as_str())),
            "title":t.title,"contract":t.contract,"purpose":t.purpose,"projects":t.projects,"assignee":t.assignee,
            "status":status,"claimable":claim_error.is_none(),"waiting_reason":claim_error,
            "assessment":crate::tasks::parent_assessment(state,key),"readiness_blocker":crate::tasks::readiness_blocker(state,key),
            "review_required":crate::tasks::review_required(state,key,t),"created_at":t.created_at,
            "claim":t.claim,"unclaimed_eligible":unclaimed_error.is_none(),"unclaimed_error":unclaimed_error,
            "origin_revision":t.origin_revision,"verification":crate::objects::verification(state,key),"signal":crate::objects::signal(state,key)
        })));
    }
    Ok(entries)
}
