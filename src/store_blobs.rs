//! Who may read an attachment. The bytes live in Spaces (crate::spaces).
use super::*;

/// The name and content type of the file `hash`, only for an actor who reads the
/// workspace where it is attached.
///
/// The hash is global: the same file can be attached in two workspaces. So the right
/// comes from the MENTION: a note of THIS workspace must name the hash. Otherwise the
/// hash would be a password, and anyone with the file can compute it.
pub async fn attachment(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    hash: &str,
) -> Result<(String, String)> {
    let conn = pool.get().await?;
    let resolved = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &resolved, actor, false).await?;
    let found = conn.query_opt("SELECT a.value->>'name',a.value->>'content_type' FROM joppa.state_entries n CROSS JOIN LATERAL jsonb_each(n.value->'attachments') a WHERE n.workspace=$1 AND n.section='notes' AND a.value->>'hash'=$2 LIMIT 1", &[&resolved,&hash]).await?.ok_or(AccessDenied)?;
    Ok((found.get(0), found.get(1)))
}

/// The files that only `workspace` names in its journal. Its deletion removes them.
pub(super) async fn orphan_files(
    conn: &impl GenericClient,
    workspace: &str,
) -> Result<Vec<String>> {
    Ok(conn
        .query(
            "SELECT DISTINCT m[1] FROM joppa.events e, regexp_matches(e.command::text, '\"hash\": ?\"([0-9a-f]{64})\"', 'g') m WHERE e.workspace=$1 AND NOT EXISTS (SELECT 1 FROM joppa.events o WHERE o.workspace<>$1 AND strpos(o.command::text, m[1])>0)",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect())
}

/// Remove the files of a deleted workspace from Spaces, after the commit. A failure keeps
/// a file that nothing names, never a record without its file.
pub(super) async fn remove_files(hashes: Vec<String>) {
    for hash in hashes {
        let removed = async { crate::spaces::storage()?.delete(&hash).await }.await;
        if let Err(e) = removed {
            eprintln!("joppa: file {hash} of a deleted workspace stays in Spaces: {e:#}");
        }
    }
}
