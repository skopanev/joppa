use super::*;

pub(super) fn validate_selection<'a>(
    state: &Workspace,
    project: Option<&str>,
    purpose: Option<&str>,
    priorities: &'a [String],
) -> Result<Vec<(&'a str, &'a str)>> {
    if let Some(project) = project {
        ensure!(state.projects.contains_key(project), "unknown project");
    }
    if let Some(purpose) = purpose {
        ensure!(
            ["implementation", "manual", "decision"].contains(&purpose),
            "unknown task purpose"
        );
    }
    priorities
        .iter()
        .map(|selector| {
            let (kind, id) = selector
                .split_once(':')
                .context("priority must be kind:id")?;
            ensure!(!id.is_empty(), "priority ID is empty");
            let exists = match kind {
                "domain" => state.domains.contains_key(id),
                "capability" => state.capabilities.contains_key(id),
                "requirement" => state.requirements.contains_key(id),
                "ac" => state.requirements.values().any(|q| {
                    q.revisions
                        .last()
                        .is_some_and(|r| r.acs.iter().any(|ac| ac.id == id))
                }),
                "task" => state
                    .requirements
                    .values()
                    .any(|q| q.revisions.last().is_some_and(|r| r.tasks.contains_key(id))),
                "project" => state.projects.contains_key(id),
                "consumer" => state.consumers.contains_key(id),
                _ => anyhow::bail!("unknown priority kind: {kind}"),
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
        .position(|(kind, target)| match *kind {
            "domain" => state
                .capabilities
                .get(&q.capability)
                .is_some_and(|c| c.domain == *target),
            "capability" => q.capability == *target,
            "requirement" => req == *target,
            "ac" => task.acs.iter().any(|ac| ac == target),
            "task" => id == *target,
            "project" => task.projects.iter().any(|project| project == target),
            "consumer" => revision.consumers.iter().any(|consumer| consumer == target),
            _ => false,
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
    Ok(state
        .requirements
        .iter()
        .filter_map(|(req, q)| q.revisions.last().map(|r| (req, q, r)))
        .flat_map(|(req, q, r)| r.tasks.iter().map(move |(id, task)| (req, q, r, id, task)))
        .filter(|(_, _, _, _, task)| {
            project.is_none_or(|p| task.projects.iter().any(|id| id == p))
                && purpose.is_none_or(|p| task.purpose == p)
        })
        .filter(|(_, _, r, _, task)| checks::claimable(state, actor, r, task).is_ok())
        .min_by_key(|(req, q, r, id, task)| {
            (
                rank(state, req, q, r, id, task, &priorities),
                task.created_at,
                (*req).clone(),
                (*id).clone(),
            )
        })
        .map(|(req, _, r, id, _)| (req.clone(), r.number, id.clone())))
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
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    let row = conn
        .query_one(
            "SELECT position,COALESCE(state->>'workspace_id',workspace),(state->'members')::text,(state->'domains')::text,(state->'capabilities')::text,(state->'consumers')::text,(state->'projects')::text,COALESCE((SELECT jsonb_object_agg(key,jsonb_build_object('capability',value->'capability','revision',value->'revisions'->-1)) FROM jsonb_each(state->'requirements')),'{}'::jsonb)::text FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    let position = row.get::<_, i64>(0) as u64;
    let public_workspace: &str = row.get(1);
    let mut state = Workspace::default();
    state.members = serde_json::from_str(row.get::<_, &str>(2))?;
    state.domains = serde_json::from_str(row.get::<_, &str>(3))?;
    state.capabilities = serde_json::from_str(row.get::<_, &str>(4))?;
    state.consumers = serde_json::from_str(row.get::<_, &str>(5))?;
    state.projects = serde_json::from_str(row.get::<_, &str>(6))?;
    let requirements: std::collections::BTreeMap<String, Value> =
        serde_json::from_str(row.get::<_, &str>(7))?;
    for (req, value) in requirements {
        state.requirements.insert(
            req,
            crate::model::Requirement {
                capability: value["capability"]
                    .as_str()
                    .context("requirement lacks capability")?
                    .into(),
                revisions: vec![serde_json::from_value(value["revision"].clone())?],
                comments: Default::default(),
            },
        );
    }
    let priorities = validate_selection(&state, project, purpose, priorities)?;
    let offset = if let Some(cursor) = cursor {
        let (snapshot, offset) = cursor.split_once(':').context("invalid task cursor")?;
        ensure!(
            snapshot.parse::<u64>()? == position,
            "task queue changed; start again without cursor"
        );
        offset.parse::<usize>()?
    } else {
        0
    };
    let mut entries = vec![];
    for (req, requirement) in &state.requirements {
        let revision = requirement.revisions.last().unwrap();
        for (id, task) in &revision.tasks {
            if !project.is_none_or(|p| task.projects.iter().any(|id| id == p))
                || !purpose.is_none_or(|p| task.purpose == p)
            {
                continue;
            }
            let claim_error = checks::claimable(&state, actor, revision, task)
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
                rank(&state, req, requirement, revision, id, task, &priorities),
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
    entries.sort_by(|a, b| (&a.0, &a.1, &a.2, &a.3).cmp(&(&b.0, &b.1, &b.2, &b.3)));
    ensure!(offset <= entries.len(), "invalid task cursor offset");
    let total = entries.len();
    let items: Vec<Value> = entries
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|(_, _, _, _, item)| item)
        .collect();
    let next =
        (offset + items.len() < total).then(|| format!("{position}:{}", offset + items.len()));
    Ok(
        json!({"workspace":public_workspace,"position":position,"view":view,"total":total,"tasks":items,"next_cursor":next}),
    )
}
