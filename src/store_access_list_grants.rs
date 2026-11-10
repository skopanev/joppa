//! Part of `store_access`: `list_grants` and the items after it, moved from store_access.rs unchanged.
use super::*;

/// The workspaces of `rows` (internal ID in column `id`) that are in the scope of `actor`, as
/// access::check and the workspace list read it. A member outside the scope gets none of them.
async fn in_scope<'a>(
    conn: &impl GenericClient,
    actor: &str,
    rows: &'a [tokio_postgres::Row],
    id: usize,
) -> Result<Vec<&'a tokio_postgres::Row>> {
    let allowed = scope(conn, actor).await?;
    Ok(rows
        .iter()
        .filter(|r| {
            allowed
                .as_ref()
                .is_none_or(|set| set.contains(&r.get::<_, String>(id)))
        })
        .collect())
}

pub(crate) async fn list_grants(
    pool: &Pool,
    actor: &str,
    table: &str,
    column: &str,
    key: &str,
) -> Result<Value> {
    let conn = pool.get().await?;
    let rows = conn
        .query(
            &format!("SELECT COALESCE(b.first_view->>'workspace_id',s.workspace), COALESCE(b.first_view->>'title',s.workspace),
                    COALESCE(array_agg(g.{column} ORDER BY g.{column}) FILTER (WHERE g.{column} IS NOT NULL), '{{}}'), s.workspace
             FROM joppa.state_entries s
             LEFT JOIN joppa.board_cache b ON b.workspace=s.workspace
             LEFT JOIN joppa.{table} g ON g.workspace=s.workspace
             WHERE s.section='meta' AND s.key='' AND s.workspace IN ({HUMAN_MEMBERSHIP}) AND s.value->'deleted_at' IS NULL
             GROUP BY 1,2,4 ORDER BY 2,1"),
            &[&actor],
        )
        .await?;
    let items: Vec<Value> = in_scope(&**conn, actor, &rows, 3)
        .await?
        .into_iter()
        .map(|r| {
            let mut item = json!({"id": r.get::<_, String>(0), "title": r.get::<_, String>(1)});
            item[key] = json!(r.get::<_, Vec<String>>(2));
            item
        })
        .collect();
    Ok(json!({"items": items}))
}

/// The workspaces where this actor is a human member and which are in its scope, each with its
/// people and its pending grants, in one read. A person shows as "name <email>", as the email,
/// or as the member ID when no Google identity is bound. Agent members are not people, so the
/// list omits them. An email grant of a person who is already a member is not listed again. A
/// hidden workspace is listed too.
pub async fn list_owned_access(pool: &Pool, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let rows = conn
        .query(
            &format!("SELECT COALESCE(b.first_view->>'workspace_id',s.workspace), COALESCE(b.first_view->>'title',s.workspace),
                    COALESCE((SELECT array_agg(COALESCE(NULLIF(i.display_name,'')||' <'||i.email||'>', i.email, m.key) ORDER BY m.key)
                              FROM jsonb_each_text(b.first_view->'members') m
                              LEFT JOIN joppa.google_identities i ON i.actor=m.key
                              WHERE m.value='human'), '{{}}'),
                    COALESCE((SELECT array_agg(m.key ORDER BY m.key)
                              FROM jsonb_each_text(b.first_view->'members') m
                              WHERE m.value='human'), '{{}}'),
                    COALESCE((SELECT array_agg(e.email ORDER BY e.email) FROM joppa.email_grants e WHERE e.workspace=s.workspace
                              AND NOT EXISTS(SELECT 1 FROM jsonb_each_text(b.first_view->'members') m
                                             JOIN joppa.google_identities i ON i.actor=m.key WHERE lower(i.email)=e.email)), '{{}}'),
                    COALESCE((SELECT array_agg(d.domain ORDER BY d.domain) FROM joppa.domain_grants d WHERE d.workspace=s.workspace), '{{}}'),
                    s.workspace
             FROM joppa.state_entries s
             LEFT JOIN joppa.board_cache b ON b.workspace=s.workspace
             WHERE s.section='meta' AND s.key='' AND s.workspace IN ({HUMAN_MEMBERSHIP}) AND s.value->'deleted_at' IS NULL
             ORDER BY 2,1"),
            &[&actor],
        )
        .await?;
    let items: Vec<Value> = in_scope(&**conn, actor, &rows, 6)
        .await?
        .into_iter()
        .map(|r| {
            // Each member has its ID, so any human member can remove it. The page leaves out
            // Remove on the row of the signed-in person.
            let labels: Vec<String> = r.get(2);
            let ids: Vec<String> = r.get(3);
            let members: Vec<Value> = ids
                .iter()
                .zip(&labels)
                .map(|(id, label)| json!({"id": id, "label": label}))
                .collect();
            json!({"id": r.get::<_, String>(0), "title": r.get::<_, String>(1),
                "people": labels, "members": members, "emails": r.get::<_, Vec<String>>(4),
                "domains": r.get::<_, Vec<String>>(5)})
        })
        .collect();
    Ok(json!({"items": items}))
}
