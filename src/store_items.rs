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

/// One page of the MCP workspace list and the total, in one statement (tac:tcospnl8). $1 is
/// the actor. $2 is the allowed set of a rules file, else null: then a bound Google identity
/// keeps the workspaces of its email and domain grants and the ones it created, as
/// access::scope does, and an unbound identity keeps each workspace where it is a member.
const PAGE: &str = "WITH i AS (SELECT lower(email) AS email, lower(hosted_domain) AS hd FROM joppa.google_identities WHERE actor=$1),
    listed AS (SELECT b.workspace, b.first_view->>'workspace_id' AS id, b.first_view->>'title' AS title, COALESCE(b.first_view->'hidden_by' ? $1,false) AS hidden
        FROM joppa.board_cache b
        WHERE b.workspace IN (SELECT workspace FROM joppa.state_entries WHERE section='members' AND key=$1) AND b.first_view->'members' ? $1 AND CASE
            WHEN $2::text[] IS NOT NULL THEN b.workspace=ANY($2)
            WHEN NOT EXISTS(SELECT 1 FROM i) THEN true
            ELSE EXISTS(SELECT 1 FROM i JOIN joppa.email_grants g ON g.email=i.email WHERE g.workspace=b.workspace)
                OR EXISTS(SELECT 1 FROM i JOIN joppa.domain_grants d ON d.domain=i.hd WHERE d.workspace=b.workspace)
                OR EXISTS(SELECT 1 FROM joppa.state_entries s WHERE s.workspace=b.workspace AND s.section='meta' AND s.key='' AND s.value->>'admin'=$1 AND s.value->'deleted_at' IS NULL) END)
    SELECT t.total, p.id, p.title, p.hidden FROM (SELECT count(*) AS total FROM listed) t
    LEFT JOIN LATERAL (SELECT id, title, hidden FROM listed ORDER BY workspace OFFSET $3 LIMIT $4) p ON true";

/// The MCP workspace list: `limit` items after `start`, each {id, title} and `hidden: true`
/// for a workspace that the actor hides. It reads no member or grant list of a workspace.
pub async fn list_page(pool: &Pool, actor: &str, start: usize, limit: usize) -> Result<Value> {
    let conn = pool.get().await?;
    // Only a rules file needs the access scope in Rust; the statement covers the grants.
    let allowed: Option<Vec<String>> = match std::env::var_os("JOPPA_ACCESS_RULES_FILE") {
        Some(_) => access::scope(&**conn, actor)
            .await?
            .map(|set| set.into_iter().collect()),
        None => None,
    };
    let (start, limit) = (start as i64, limit as i64);
    let rows = conn
        .query(PAGE, &[&actor, &allowed, &start, &limit])
        .await?;
    let total: i64 = rows.first().map_or(0, |row| row.get(0));
    let mut items = vec![];
    for row in &rows {
        let Some(id) = row.get::<_, Option<String>>(1) else {
            continue;
        };
        let mut item = json!({"id": id, "title": row.get::<_, Option<String>>(2)});
        if row.get::<_, Option<bool>>(3) == Some(true) {
            item["hidden"] = json!(true);
        }
        items.push(item);
    }
    let mut reply = json!({"items": items, "total": total});
    if start + limit < total {
        reply["next_cursor"] = json!((start + limit).to_string());
    }
    Ok(reply)
}

/// The web list: the listed workspaces without the hidden ones.
pub fn visible(list: Value) -> Value {
    let items = list.as_array().into_iter().flatten();
    json!(items.filter(|w| w["hidden"] != true).collect::<Vec<_>>())
}
