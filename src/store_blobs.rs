//! Who may read and write the files of a workspace. The bytes live in Spaces (crate::spaces),
//! in the folder `workspaces/<workspace>/` of the resolved workspace ID.
use super::*;
use std::collections::BTreeMap;

/// The resolved ID of `workspace` for an actor who can read it: the name of its file folder.
pub async fn file_workspace(pool: &Pool, workspace: &str, actor: &str) -> Result<String> {
    let conn = pool.get().await?;
    let resolved = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &resolved, actor, false).await?;
    Ok(resolved)
}

/// The attachments of the object `target` for an actor who reads the workspace, without the
/// object read (tac:jogrtcra): `{address, kind, folder, attachments}`. `folder` is the
/// resolved workspace ID, and `attachments` the map of id to file. An alias reads its target.
pub async fn object_files(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    target: &str,
) -> Result<Value> {
    let conn = pool.get().await?;
    let folder = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &folder, actor, false).await?;
    let row = conn.query_opt("SELECT o.address,o.kind,(SELECT (n.value->'attachments')::text FROM joppa.state_entries n WHERE n.workspace=o.workspace AND n.section='notes' AND n.key=COALESCE(o.process->>'annotation_address',o.address)) FROM joppa.objects o WHERE o.workspace=$1 AND o.address=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2)", &[&folder, &target]).await?.context("unknown object")?;
    let attachments: Option<String> = row.get(2);
    let attachments: Value = serde_json::from_str(attachments.as_deref().unwrap_or("{}"))?;
    Ok(
        json!({"address": row.get::<_, String>(0), "kind": row.get::<_, String>(1),
        "folder": folder, "attachments": attachments}),
    )
}

/// The Spaces key, name and content type of the file `hash`, only for an actor who reads
/// the workspace, and only when an active or <14-day detached attachment names the hash.
pub async fn attachment(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    hash: &str,
) -> Result<(String, String, String)> {
    let resolved = file_workspace(pool, workspace, actor).await?;
    let conn = pool.get().await?;
    let found = conn.query_opt("SELECT a.value->>'name',a.value->>'content_type' FROM joppa.state_entries n CROSS JOIN LATERAL jsonb_each(n.value->'attachments') a WHERE n.workspace=$1 AND n.section='notes' AND a.value->>'hash'=$2 AND (NOT COALESCE((a.value->>'removed')::boolean,false) OR NULLIF(a.value->>'removed_at','') IS NULL OR (a.value->>'removed_at')::timestamptz > clock_timestamp() - interval '14 days') ORDER BY COALESCE((a.value->>'removed')::boolean,false) ASC LIMIT 1", &[&resolved,&hash]).await?.ok_or(AccessDenied)?;
    let key = crate::spaces::Spaces::key(&resolved, hash)?;
    Ok((key, found.get(0), found.get(1)))
}

/// The actor and the file hash of the stored request `request_id` of the resolved
/// `workspace`, or None for a new request. An attach checks it BEFORE the upload, so a
/// reused request_id with other bytes stores nothing. The hash is empty for another op.
pub async fn receipt_hash(
    pool: &Pool,
    workspace: &str,
    request_id: &str,
) -> Result<Option<(String, String)>> {
    let row = pool.get().await?.query_opt("SELECT actor,COALESCE(input->'operations'->0->'command'->>'hash',input->'legacy_command'->>'hash','') FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2", &[&workspace,&request_id]).await?;
    Ok(row.map(|row| (row.get(0), row.get(1))))
}

/// Delete files in Spaces whose only attachments in the workspace were detached at least
/// 14 days ago. Returns the number of files deleted from Spaces.
pub async fn sweep_detached(pool: &Pool) -> Result<usize> {
    let Ok(storage) = crate::spaces::storage() else {
        return Ok(0);
    };
    let rows = pool.get().await?.query(
        "SELECT n.workspace,a.value->>'hash' FROM joppa.state_entries n CROSS JOIN LATERAL jsonb_each(n.value->'attachments') a WHERE n.section='notes' GROUP BY n.workspace,a.value->>'hash' HAVING bool_and(COALESCE((a.value->>'removed')::boolean,false) AND NULLIF(a.value->>'removed_at','') IS NOT NULL) AND max((a.value->>'removed_at')::timestamptz) <= clock_timestamp() - interval '14 days'",
        &[],
    ).await?;
    let mut by_ws: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in rows {
        by_ws.entry(row.get(0)).or_default().push(row.get(1));
    }
    let mut removed = 0;
    for (ws, hashes) in by_ws {
        let prefix = crate::spaces::Spaces::folder(&ws)?;
        let present = storage.list(&prefix).await?;
        for hash in hashes {
            let key = crate::spaces::Spaces::key(&ws, &hash)?;
            if present.contains(&key) {
                storage.delete(&key).await?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Remove the file folder of a deleted workspace, after the commit. A failure keeps files
/// that nothing names, never a record without its file.
pub(super) async fn remove_folder(workspace: &str) {
    let removed = async { crate::spaces::storage()?.delete_folder(workspace).await }.await;
    if let Err(e) = removed {
        eprintln!("joppa: the file folder of deleted workspace {workspace} stays in Spaces: {e:#}");
    }
}
