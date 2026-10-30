//! Who may read and write the files of a workspace. The bytes live in Spaces (crate::spaces),
//! in the folder `workspaces/<workspace>/` of the resolved workspace ID.
use super::*;

/// The resolved ID of `workspace` for an actor who can read it: the name of its file folder.
pub async fn file_workspace(pool: &Pool, workspace: &str, actor: &str) -> Result<String> {
    let conn = pool.get().await?;
    let resolved = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &resolved, actor, false).await?;
    Ok(resolved)
}

/// The Spaces key, name and content type of the file `hash`, only for an actor who reads
/// the workspace, and only when a note of THIS workspace names the hash.
pub async fn attachment(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    hash: &str,
) -> Result<(String, String, String)> {
    let resolved = file_workspace(pool, workspace, actor).await?;
    let conn = pool.get().await?;
    let found = conn.query_opt("SELECT a.value->>'name',a.value->>'content_type' FROM joppa.state_entries n CROSS JOIN LATERAL jsonb_each(n.value->'attachments') a WHERE n.workspace=$1 AND n.section='notes' AND a.value->>'hash'=$2 LIMIT 1", &[&resolved,&hash]).await?.ok_or(AccessDenied)?;
    let key = crate::spaces::Spaces::key(&resolved, hash)?;
    Ok((key, found.get(0), found.get(1)))
}

/// Remove the file folder of a deleted workspace, after the commit. A failure keeps files
/// that nothing names, never a record without its file.
pub(super) async fn remove_folder(workspace: &str) {
    let removed = async { crate::spaces::storage()?.delete_folder(workspace).await }.await;
    if let Err(e) = removed {
        eprintln!("joppa: the file folder of deleted workspace {workspace} stays in Spaces: {e:#}");
    }
}
