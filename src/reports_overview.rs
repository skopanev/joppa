//! Test status reads for people and agents: the overview, a Task and one criterion.
use super::*;

/// Green and red criteria with their ancestors, so each row can show "green / total" below it.
pub async fn tests_overview(pool: &Pool, workspace: &str, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    let listed: Vec<String> = conn
        .query(
            "SELECT key FROM joppa.state_entries WHERE workspace=$1 AND section='ac_tests'",
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

/// A task has no test status of its own. Only complete_task sets done; verification
/// follows the status of its criterion.
pub async fn task_status(pool: &Pool, workspace: &str, actor: &str, task: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    let row = conn
        .query_opt(
            "SELECT o.parent,COALESCE(jsonb_typeof(o.process->'done'),'null')='string' FROM joppa.objects o JOIN joppa.board_cache b ON b.workspace=o.workspace WHERE o.workspace=$1 AND o.address=$2 AND b.first_view->'object_types'->o.kind->>'role'='task'",
            &[&workspace, &task],
        )
        .await?;
    let (criterion, done): (String, bool) = row
        .and_then(|row| Some((row.get::<_, Option<String>>(0)?, row.get(1))))
        .context("unknown task")?;
    drop(conn);
    let status = criterion_tests(pool, &workspace, actor, &criterion).await?;
    // Two independent values: done is the complete_task evidence in the Task process,
    // verification is the criterion status. A green criterion does not complete a Task.
    let verification = match status["status"].as_str() {
        Some("green") => "passed",
        Some("red") => "failed",
        _ => "unverified",
    };
    Ok(json!({
        "address": task,
        "verification": verification,
        "done": done,
        "criterion": {"address": criterion, "status": status["status"], "commit": status["commit"]},
    }))
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
