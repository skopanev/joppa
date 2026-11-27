//! The public workspace name and the journal position of one read snapshot, so each list
//! and detail read names the workspace and the position that its rows belong to.
use super::*;

/// Read inside the transaction of the rows, so the position matches them.
pub(crate) async fn head(client: &impl GenericClient, workspace: &str) -> Result<(String, i64)> {
    let row = client
        .query_one(
            "SELECT COALESCE(state->>'workspace_id',workspace),position FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    Ok((row.get(0), row.get(1)))
}

/// The public workspace ID after a write, or `workspace` when none is set.
pub async fn public_id(pool: &Pool, workspace: &str) -> Result<String> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    Ok(head(&**conn, &workspace).await?.0)
}
