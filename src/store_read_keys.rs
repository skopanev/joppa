//! The read key of a workspace (migration 068). A human member creates, replaces and revokes
//! it from a signed-in browser. The table keeps only the SHA-256 hex digest of the key.
use super::*;

/// The form of `created_at` in each reply: UTC with milliseconds.
const CREATED_AT: &str =
    "to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')";

/// The workspace when `actor` is a human member of it. Any other refusal is AccessDenied, and
/// a database fault stays a fault.
async fn human_workspace(pool: &Pool, workspace: &str, actor: &str) -> Result<String> {
    match access::human_member_of(pool, workspace, actor).await {
        Ok((workspace, _)) => Ok(workspace),
        Err(e) if is_fault(&e) => Err(e),
        Err(_) => Err(AccessDenied.into()),
    }
}

fn is_fault(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| {
        cause.is::<tokio_postgres::Error>() || cause.is::<deadpool_postgres::PoolError>()
    })
}

/// `{"active", "created_at"}` of the read key of the workspace. It never gives the key.
pub async fn read_key(pool: &Pool, workspace: &str, actor: &str) -> Result<Value> {
    let workspace = human_workspace(pool, workspace, actor).await?;
    let sql = format!("SELECT {CREATED_AT} FROM joppa.workspace_read_keys WHERE workspace=$1");
    let row = pool.get().await?.query_opt(&sql, &[&workspace]).await?;
    let created_at = row.map(|r| r.get::<_, String>(0));
    Ok(json!({"active": created_at.is_some(), "created_at": created_at}))
}

/// Store `digest` as the only read key of the workspace. A previous key stops at once. It
/// gives `created_at`.
pub async fn set_read_key(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    digest: &str,
) -> Result<String> {
    let workspace = human_workspace(pool, workspace, actor).await?;
    let sql = format!(
        "INSERT INTO joppa.workspace_read_keys(workspace,token_sha256,created_by) VALUES($1,$2,$3)
        ON CONFLICT (workspace) DO UPDATE SET token_sha256=EXCLUDED.token_sha256,
            created_by=EXCLUDED.created_by,created_at=now()
        RETURNING {CREATED_AT}"
    );
    let conn = pool.get().await?;
    let row = conn.query_one(&sql, &[&workspace, &digest, &actor]).await?;
    Ok(row.get(0))
}

/// Delete the read key of the workspace.
pub async fn revoke_read_key(pool: &Pool, workspace: &str, actor: &str) -> Result<Value> {
    let workspace = human_workspace(pool, workspace, actor).await?;
    let sql = "DELETE FROM joppa.workspace_read_keys WHERE workspace=$1";
    pool.get().await?.execute(sql, &[&workspace]).await?;
    Ok(json!({"active": false}))
}

/// The workspace of a read key digest, or None for an unknown or revoked key.
pub async fn read_key_workspace(pool: &Pool, digest: &str) -> Result<Option<String>> {
    let sql = "SELECT workspace FROM joppa.workspace_read_keys WHERE token_sha256=$1";
    let row = pool.get().await?.query_opt(sql, &[&digest]).await?;
    Ok(row.map(|r| r.get(0)))
}
