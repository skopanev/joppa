//! Test status reads for people and agents: the overview, a Task and one criterion.
use super::*;

/// Green and red criteria with their ancestors, so each row can show "green / total" below it.
pub async fn tests_overview(pool: &Pool, workspace: &str, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    // A missing or stale projection is an error here as in every other read.
    criterion_test_statuses(&**conn, &workspace, &[], &Default::default()).await?;
    // One query: each present criterion with tests that is green or red, from the stored
    // status, with its ancestors from the parent up. Archived criteria count nowhere.
    let rows = conn
        .query(
            "WITH RECURSIVE listed AS (
                SELECT address,test_status->>'status' AS status,parent FROM joppa.objects
                WHERE workspace=$1 AND present AND jsonb_array_length(COALESCE(test_status->'tests','[]'::jsonb))>0
                    AND test_status->>'status' IN ('green','red')
            ), up(address,node,depth,path) AS (
                SELECT address,parent,1,ARRAY[]::text[] FROM listed WHERE parent IS NOT NULL
                UNION ALL
                SELECT u.address,o.parent,u.depth+1,u.path||u.node FROM up u
                JOIN joppa.objects o ON o.workspace=$1 AND o.address=u.node AND o.present
                WHERE o.parent IS NOT NULL AND NOT o.parent=ANY(u.path||u.node)
            )
            SELECT l.address,l.status,COALESCE((SELECT array_agg(u.node ORDER BY u.depth) FROM up u WHERE u.address=l.address),ARRAY[]::text[])
            FROM listed l ORDER BY l.address",
            &[&workspace],
        )
        .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| json!({"address": r.get::<_, String>(0), "status": r.get::<_, String>(1), "ancestors": r.get::<_, Vec<String>>(2)}))
        .collect();
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
