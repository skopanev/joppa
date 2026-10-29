//! The workspace list and the launch gate.
use super::*;

pub async fn list(pool: &Pool, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let allowed = access::scope(&**conn, actor).await?;
    let rows = conn
        .query(
            &format!("{} ORDER BY workspace", access::MEMBERSHIP),
            &[&actor],
        )
        .await?;
    let mut items = vec![];
    for row in rows {
        let id: String = row.get(0);
        if allowed.as_ref().is_some_and(|set| !set.contains(&id)) {
            continue;
        }
        if let Some(card) = conn.query_opt("SELECT first_view->>'workspace_id',first_view->>'title' FROM joppa.board_cache WHERE workspace=$1 AND first_view->'members' ? $2", &[&id,&actor]).await? {
            items.push(json!({"id": card.get::<_,String>(0), "title": card.get::<_,String>(1)}));
        }
    }
    Ok(json!(items))
}

/// The release caller supplies the exact commit. An acceptance on another commit never
/// authorizes this build.
pub fn gate(card: &Value, commit: &str) -> Result<usize> {
    let state: Workspace = serde_json::from_value(card["state"].clone())?;
    crate::launch_gate::check(&state, commit)
}
