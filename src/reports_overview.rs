//! Test status reads for people and agents: the overview, a Task and one criterion.
use super::*;

/// Green and red criteria with their ancestors, so each row can show "green / total" below it.
pub async fn tests_overview(pool: &Pool, workspace: &str, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    let listed: Vec<String> = conn
        .query(
            "SELECT address FROM joppa.objects WHERE workspace=$1 AND present AND jsonb_array_length(COALESCE(test_status->'tests','[]'::jsonb))>0",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let parents: std::collections::HashMap<String, Option<String>> = conn
        .query(
            "SELECT address,parent FROM joppa.objects WHERE workspace=$1 AND present",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    drop(conn);
    let mut items = vec![];
    // Archived criteria are out of the spec; they count nowhere.
    for address in listed.into_iter().filter(|a| parents.contains_key(a)) {
        let status = criterion_tests(pool, &workspace, actor, &address).await?;
        if !["green", "red"].contains(&status["status"].as_str().unwrap_or_default()) {
            continue;
        }
        let mut ancestors = vec![];
        let mut next = parents.get(&address).cloned().flatten();
        while let Some(parent) = next.filter(|p| !ancestors.contains(p)) {
            next = parents.get(&parent).cloned().flatten();
            ancestors.push(parent);
        }
        items.push(json!({"address": address, "status": status["status"], "ancestors": ancestors}));
    }
    Ok(json!({"items": items}))
}

/// A Task has no test status of its own. Only complete_task sets done and names the git
/// sha where the Task is ready.
pub async fn task_status(pool: &Pool, workspace: &str, actor: &str, task: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    // A missing or stale projection is an error here as in every other read.
    criterion_test_statuses(&**conn, &workspace, &[], &Default::default()).await?;
    let row = conn
        .query_opt(
            "SELECT COALESCE(jsonb_typeof(o.process->'done'),'null')='string',o.process->>'commit' FROM joppa.objects o JOIN joppa.board_cache b ON b.workspace=o.workspace WHERE o.workspace=$1 AND o.address=$2 AND b.first_view->'object_types'->o.kind->>'role'='task'",
            &[&workspace, &task],
        )
        .await?
        .context("unknown task")?;
    let (done, commit): (bool, Option<String>) = (row.get(0), row.get(1));
    Ok(json!({"address": task, "done": done, "commit": commit}))
}

/// Status of one criterion from its saved test list and all clean default-branch reports.
pub async fn criterion_tests(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    target: &str,
) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    criterion_test_statuses(
        &**conn,
        &workspace,
        &[target.to_owned()],
        &Default::default(),
    )
    .await?
    .remove(target)
    .with_context(|| format!("unknown criterion {target}"))
}
