//! The reply of one search: the counts, the word and exact matches, the nearest vectors, their
//! fusion and the page of results, in one repeatable-read transaction.
use super::*;

/// The reply of one search. With semantic_status exact_match, only an exact match answers:
/// without one the reply is None.
pub(super) async fn answer(
    pool: &Pool,
    actor: &str,
    scope: &[String],
    (query, filter, limit, flags): (&str, &crate::node_filter::NodeFilter, i64, Flags),
    vector: Option<String>,
    mut semantic: &'static str,
) -> Result<Option<Value>> {
    let home = &scope[0];
    let mut conn = pool.get().await?;
    let tx = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    // Recheck after network I/O: access may have been revoked while embedding.
    for ws in scope {
        access::check(&*tx, ws, actor, false).await?;
    }
    let state = tx
        .query_one(
            "SELECT canonical_id,position FROM joppa.search_state WHERE workspace=$1",
            &[home],
        )
        .await?;
    let canonical: String = state.get(0);
    let position: i64 = state.get(1);
    // The node filters keep the same objects in the counts and in each candidate list.
    let bound = crate::node_filter::bind(&*tx, home, filter, false, flags.all_versions).await?;
    type Key = (String, String, String, i32);
    let mut scores = BTreeMap::<Key, f64>::new();
    // (exact ID or address, matched by item ID): a kept item ID is reported as queried.
    let mut exact = BTreeMap::<Key, (bool, bool)>::new();
    let rows = candidates::words(&tx, scope, query, flags.all_versions, &bound).await?;
    if semantic == "exact_match" && !rows.iter().any(|row| row.get::<_, bool>(4)) {
        return Ok(None);
    }
    let (total, embedded) = candidates::counts(&tx, scope, flags.all_versions, &bound).await?;
    if semantic == "ready" && embedded < total {
        semantic = "pending";
    }
    for (rank, row) in rows.iter().enumerate() {
        let key: Key = (row.get(0), row.get(1), row.get(2), row.get(3));
        let is_exact: bool = row.get(4);
        scores.insert(
            key.clone(),
            if is_exact {
                1.0
            } else {
                1.0 / (61.0 + rank as f64)
            },
        );
        exact.insert(key, (is_exact, row.get(5)));
    }
    if let Some(vector) = &vector {
        let rows = candidates::nearest(&tx, scope, vector, flags.all_versions, &bound).await?;
        for (rank, row) in rows.iter().enumerate() {
            *scores
                .entry((row.get(0), row.get(1), row.get(2), row.get(3)))
                .or_default() += 1.0 / (61.0 + rank as f64);
        }
    }
    let mut ranked = scores.into_iter().collect::<Vec<_>>();
    // A copied item ID or an address names its object: the reply gives only the exact matches.
    if ranked
        .iter()
        .any(|(key, _)| exact.get(key).is_some_and(|e| e.0))
    {
        ranked.retain(|(key, _)| exact.get(key).is_some_and(|e| e.0));
    }
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let ranked_total = ranked.len();
    let next_cursor = (flags.offset + (limit as usize) < ranked_total)
        .then(|| (flags.offset + limit as usize).to_string());
    let selected = ranked
        .into_iter()
        .skip(flags.offset)
        .take(limit as usize)
        .map(|(key, score)| {
            let (is_exact, item) = exact.get(&key).copied().unwrap_or_default();
            let (workspace, kind, id, rev) = key;
            json!({"workspace":workspace,"kind":kind,"id":id,"rev":rev,"score":score,"exact":is_exact,"item":item})
        })
        .collect::<Vec<_>>();
    let rows = tx.query("SELECT s.canonical_id,k.kind,k.id,COALESCE(d.title,r.title),(CASE WHEN k.rev>=0 THEN v.document->'data' ELSE o.document->'data' END)::text,d.req,CASE WHEN k.rev>=0 THEN k.rev ELSE o.revision END,i.id,k.score,k.exact,k.item,
        k.rev>=0 AND o.revision IS DISTINCT FROM k.rev,
        k.rev>=0 AND NOT EXISTS(SELECT 1 FROM joppa.search_documents x WHERE x.workspace=k.workspace AND x.kind=k.kind AND x.local_id=k.id)
      FROM jsonb_to_recordset($1::text::jsonb) AS k(workspace text,kind text,id text,rev int,score double precision,exact boolean,item boolean)
      JOIN joppa.search_state s ON s.workspace=k.workspace
      LEFT JOIN joppa.search_documents d ON k.rev<0 AND d.workspace=k.workspace AND d.kind=k.kind AND d.local_id=k.id
      LEFT JOIN joppa.search_revisions r ON k.rev>=0 AND r.workspace=k.workspace AND r.address=k.kind||':'||k.id AND r.revision=k.rev
      LEFT JOIN joppa.objects o ON o.workspace=k.workspace AND o.address=k.kind||':'||k.id
      LEFT JOIN joppa.object_revisions v ON k.rev>=0 AND v.workspace=k.workspace AND v.address=k.kind||':'||k.id AND v.revision=k.rev
      LEFT JOIN joppa.object_aliases a ON a.workspace=k.workspace AND a.target=k.kind||':'||k.id
      JOIN joppa.item_ids i ON i.workspace=k.workspace AND i.kind=COALESCE(split_part(a.address,':',1),k.kind) AND i.local_id=COALESCE(split_part(a.address,':',2),k.id) AND i.namespace=s.reference_prefix
      ORDER BY k.score DESC,k.workspace,k.kind,k.id,k.rev", &[&serde_json::to_string(&selected)?]).await?;
    let items = rows
        .iter()
        .map(|row| {
            json!({
                "workspace": row.get::<_, String>(0),
                "kind": row.get::<_, String>(1),
                "id": row.get::<_, String>(2),
                "system_id": if row.get(10) { query.to_owned() } else { row.get::<_, String>(7) },
                "title": row.get::<_, String>(3),
                "excerpt": excerpt(row.get::<_, Option<String>>(4)),
                "req": row.get::<_, Option<String>>(5),
                "revision": row.get::<_, Option<i32>>(6),
                "score": row.get::<_, f64>(8),
                "exact_id": row.get::<_, bool>(9),
                "historical": row.get::<_, bool>(11),
                "archived": row.get::<_, bool>(12),
            })
        })
        .collect::<Vec<_>>();
    let mut workspaces = vec![];
    for ws in scope {
        workspaces.push(
            tx.query_one(
                "SELECT canonical_id FROM joppa.search_state WHERE workspace=$1",
                &[ws],
            )
            .await?
            .get::<_, String>(0),
        );
    }
    tx.commit().await?;
    Ok(Some(json!({
        "workspace": canonical,
        "position": position,
        "mode": if vector.is_some() && embedded > 0 { "hybrid" } else { "full_text" },
        "semantic_status": semantic,
        "indexed": total,
        "embedded": embedded,
        "all_versions": flags.all_versions,
        "all_workspaces": flags.all_workspaces,
        "workspaces": workspaces,
        "total": ranked_total,
        "items": items,
        "next_cursor": next_cursor,
    })))
}
