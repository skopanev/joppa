use super::*;

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
            "SELECT position,(state->'members')::text,COALESCE((SELECT jsonb_object_agg(key,value->'revisions'->-1) FROM jsonb_each(state->'requirements')),'{}'::jsonb)::text FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    let position = row.get::<_, i64>(0) as u64;
    let mut state = Workspace::default();
    state.members = serde_json::from_str(row.get::<_, &str>(1))?;
    let revisions: std::collections::BTreeMap<String, crate::model::Revision> =
        serde_json::from_str(row.get::<_, &str>(2))?;
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
    for (req, revision) in &revisions {
        for (id, task) in &revision.tasks {
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
    entries.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
    ensure!(offset <= entries.len(), "invalid task cursor offset");
    let total = entries.len();
    let items: Vec<Value> = entries
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|(_, _, _, item)| item)
        .collect();
    let next =
        (offset + items.len() < total).then(|| format!("{position}:{}", offset + items.len()));
    Ok(
        json!({"workspace":workspace,"position":position,"view":view,"total":total,"tasks":items,"next_cursor":next}),
    )
}
