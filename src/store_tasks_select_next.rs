//! Part of `store_tasks`: `select_next` and the items after it, moved from store_tasks.rs unchanged.
use super::*;

/// The next claimable Task. A Task under waiting work is skipped; when only waiting
/// Tasks match, the refusal names the dependency of the first one.
pub(crate) fn select_next(
    state: &Workspace,
    actor: &str,
    project: Option<&str>,
    purpose: Option<&str>,
    priorities: &[String],
    waits: &std::collections::BTreeMap<String, String>,
) -> Result<Option<(String, u32, String)>> {
    let priorities = validate_selection(state, project, purpose, priorities)?;
    let mut waiting = None;
    let mut choices: Vec<(usize, Vec<usize>, u64, String, String, u32)> = state
        .requirements
        .iter()
        .filter_map(|(req, q)| q.revisions.last().map(|r| (req, q, r)))
        .flat_map(|(req, q, r)| r.tasks.iter().map(move |(id, t)| (req, q, r, id, t)))
        .filter(|(req, _, _, id, _)| !state.task_process.contains_key(&format!("task:{req}/{id}")))
        .filter(|(req, _, _, id, _)| !waits.contains_key(&format!("task:{req}/{id}")))
        .filter(|(_, _, _, _, t)| {
            project.is_none_or(|p| t.projects.iter().any(|id| id == p))
                && purpose.is_none_or(|p| t.purpose == p)
        })
        .filter(|(req, _, r, id, t)| checks::claimable(state, actor, req, id, r, t).is_ok())
        .map(|(req, q, r, id, t)| {
            (
                rank(state, req, q, r, id, t, &priorities),
                Vec::new(),
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
        if let Some(target) = waits.get(key) {
            waiting.get_or_insert_with(|| crate::dependencies::reason(target));
            continue;
        }
        choices.push((
            object_rank(state, key, t, &priorities),
            crate::objects::priority_chain(state, key),
            t.created_at,
            String::new(),
            key.clone(),
            state.objects[key].current().revision,
        ));
    }
    ensure!(!choices.is_empty() || !blocked, "{}", crate::tasks::BLOCKED);
    if let Some(reason) = waiting.filter(|_| choices.is_empty()) {
        anyhow::bail!("{reason}");
    }
    // Explicit priorities first, then the priority chain from the top, then journal order.
    choices.sort_by(|a, b| (&a.0, &a.1, &a.2, &a.3, &a.4).cmp(&(&b.0, &b.1, &b.2, &b.3, &b.4)));
    Ok(choices
        .into_iter()
        .next()
        .map(|(_, _, _, req, id, revision)| (req, revision, id)))
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

pub(crate) type QueueEntry = (usize, u64, String, String, Value);

pub(crate) fn queue_entries(
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
                    "confirmed":revision.confirmed,"created_at":task.created_at,
                    "claim_req":crate::tasks::requirement(state,&format!("task:{req}/{id}"))
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
            "origin_revision":t.origin_revision,"verification":(!state.test_rules_migrated).then(|| crate::objects::verification(state,key)),"signal":crate::objects::signal(state,key),
            "claim_req":crate::tasks::requirement(state,key)
        })));
    }
    Ok(entries)
}
