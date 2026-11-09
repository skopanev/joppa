//! The workspace list.
use super::*;

/// The workspaces that the actor can read. One that the actor hid gives `"hidden": true`: MCP
/// and the CLI list it, and the web list leaves it out (`visible`).
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
        if let Some(card) = conn.query_opt("SELECT first_view->>'workspace_id',first_view->>'title',COALESCE(first_view->'hidden_by' ? $2,false) FROM joppa.board_cache WHERE workspace=$1 AND first_view->'members' ? $2", &[&id,&actor]).await? {
            let mut item = json!({"id": card.get::<_,String>(0), "title": card.get::<_,String>(1)});
            if card.get::<_, bool>(2) {
                item["hidden"] = json!(true);
            }
            items.push(item);
        }
    }
    Ok(json!(items))
}

/// The web list: the listed workspaces without the hidden ones.
pub fn visible(list: Value) -> Value {
    let items = list.as_array().into_iter().flatten();
    json!(items.filter(|w| w["hidden"] != true).collect::<Vec<_>>())
}
