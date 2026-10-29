//! Part of `store_access`: `list_grants` and the items after it, moved from store_access.rs unchanged.
use super::*;

pub(crate) async fn list_grants(
    pool: &Pool,
    actor: &str,
    table: &str,
    column: &str,
    key: &str,
) -> Result<Value> {
    let conn = pool.get().await?;
    let items: Vec<Value> = conn
        .query(
            &format!("SELECT COALESCE(b.first_view->>'workspace_id',s.workspace), COALESCE(b.first_view->>'title',s.workspace),
                    COALESCE(array_agg(g.{column} ORDER BY g.{column}) FILTER (WHERE g.{column} IS NOT NULL), '{{}}')
             FROM joppa.state_entries s
             LEFT JOIN joppa.board_cache b ON b.workspace=s.workspace
             LEFT JOIN joppa.{table} g ON g.workspace=s.workspace
             WHERE s.section='meta' AND s.key='' AND s.value->>'admin'=$1 AND s.value->'deleted_at' IS NULL
             GROUP BY 1,2 ORDER BY 2,1"),
            &[&actor],
        )
        .await?
        .iter()
        .map(|r| {
            let mut item = json!({"id": r.get::<_, String>(0), "title": r.get::<_, String>(1)});
            item[key] = json!(r.get::<_, Vec<String>>(2));
            item
        })
        .collect();
    Ok(json!({"items": items}))
}
