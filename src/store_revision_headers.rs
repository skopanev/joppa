//! The revision headers of one object: number, time, actor and reason, without the bodies.
//! The caller has read the object with the same actor, so access is already checked.
use super::*;

pub async fn revision_headers(pool: &Pool, workspace: &str, address: &str) -> Result<Vec<Value>> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    let rows = conn
        .query(
            "SELECT revision,document->>'at',document->>'actor',document->>'reason' FROM joppa.object_revisions WHERE workspace=$1 AND address=$2 ORDER BY revision",
            &[&workspace, &address],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({"revision": r.get::<_, i32>(0), "at": r.get::<_, Option<String>>(1),
                "actor": r.get::<_, Option<String>>(2), "reason": r.get::<_, Option<String>>(3)})
        })
        .collect())
}
