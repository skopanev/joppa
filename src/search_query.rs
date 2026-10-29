//! The search query. Full-text and vector candidates come from the current documents of the
//! workspaces in scope, and with all_versions also from each object revision. Reciprocal
//! rank with the constant 60 fuses them. An exact item ID or address gives only its matches.
//! A candidate key is (workspace, kind, id, revision), where revision -1 is the current
//! document. A revision row that the current document already covers is not a candidate.
use super::*;
use crate::embeddings::{self, Embedder, PROFILE};
use std::collections::BTreeMap;

/// The scope of a search. Both flags are false by default. `offset` skips that many ranked
/// results, so a cursor continues the same ranking.
#[derive(Clone, Copy, Debug, Default)]
pub struct Flags {
    pub all_versions: bool,
    pub all_workspaces: bool,
    pub offset: usize,
}

const COVERED: &str = "EXISTS(SELECT 1 FROM joppa.search_documents c JOIN joppa.objects o ON o.workspace=c.workspace AND o.address=c.kind||':'||c.local_id WHERE c.workspace=r.workspace AND c.kind||':'||c.local_id=r.address AND o.revision=r.revision)";

pub async fn find(
    pool: &Pool,
    actor: &str,
    workspace: &str,
    query: &str,
    kind: Option<&str>,
    limit: i64,
    embedder: Option<&Embedder>,
) -> Result<Value> {
    find_with(
        pool,
        actor,
        workspace,
        query,
        kind,
        limit,
        embedder,
        Flags::default(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn find_with(
    pool: &Pool,
    actor: &str,
    workspace: &str,
    query: &str,
    kind: Option<&str>,
    limit: i64,
    embedder: Option<&Embedder>,
    flags: Flags,
) -> Result<Value> {
    ensure!(
        !query.trim().is_empty() && query.chars().count() <= 2000,
        "query must contain 1–2000 characters"
    );
    ensure!((1..=50).contains(&limit), "limit must be 1–50");
    ensure!(
        kind.is_none_or(|v| !v.is_empty() && v.len() <= 100),
        "unknown search kind"
    );
    // Authorization precedes any query embedding or provider traffic.
    let home = super::search::current(pool, workspace, Some(actor)).await?;
    let mut scope = vec![home.clone()];
    if flags.all_workspaces {
        let listed = super::list(pool, actor).await?;
        for item in listed.as_array().into_iter().flatten() {
            let Some(id) = item["id"].as_str() else {
                continue;
            };
            if let Ok(other) = super::search::current(pool, id, Some(actor)).await
                && !scope.contains(&other)
            {
                scope.push(other);
            }
        }
    }
    let (vector, mut semantic) = match embedder {
        Some(embedder) => match embedder.embed(&[format!("Instruct: Given a product question, retrieve product requirements and acceptance criteria that answer it.\nQuery: {query}")]).await {
            Ok(v) => (Some(embeddings::literal(&v[0])?), "ready"),
            Err(_) => (None, "unavailable"),
        },
        None => (None, "not_configured"),
    };
    let mut conn = pool.get().await?;
    let tx = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .start()
        .await?;
    // Recheck after network I/O: access may have been revoked while embedding.
    for ws in &scope {
        access::check(&*tx, ws, actor, false).await?;
    }
    let state = tx
        .query_one(
            "SELECT canonical_id,position FROM joppa.search_state WHERE workspace=$1",
            &[&home],
        )
        .await?;
    let canonical: String = state.get(0);
    let position: i64 = state.get(1);
    let counts = tx.query_one(&format!("SELECT (SELECT count(*) FROM joppa.search_documents WHERE workspace=ANY($1) AND ($2::text IS NULL OR kind=$2))
        + CASE WHEN $4 THEN (SELECT count(*) FROM joppa.search_revisions r WHERE workspace=ANY($1) AND ($2::text IS NULL OR kind=$2) AND NOT {COVERED}) ELSE 0 END,
        (SELECT count(*) FROM joppa.search_documents WHERE workspace=ANY($1) AND ($2::text IS NULL OR kind=$2) AND embedding IS NOT NULL AND embedding_profile=$3)
        + CASE WHEN $4 THEN (SELECT count(*) FROM joppa.search_revisions r WHERE workspace=ANY($1) AND ($2::text IS NULL OR kind=$2) AND embedding IS NOT NULL AND embedding_profile=$3 AND NOT {COVERED}) ELSE 0 END"),
        &[&scope, &kind, &PROFILE, &flags.all_versions]).await?;
    let total: i64 = counts.get(0);
    let embedded: i64 = counts.get(1);
    if semantic == "ready" && embedded < total {
        semantic = "pending";
    }
    type Key = (String, String, String, i32);
    let mut scores = BTreeMap::<Key, f64>::new();
    // (exact ID or address, matched by item ID): a kept item ID is reported as queried.
    let mut exact = BTreeMap::<Key, (bool, bool)>::new();
    let rows = tx.query(&format!("WITH q AS (SELECT websearch_to_tsquery('simple',$2) || websearch_to_tsquery('english',$2) || websearch_to_tsquery('russian',$2) AS terms), matches AS (
      SELECT d.workspace,d.kind,d.local_id,-1 AS rev,false AS exact,false AS item,ts_rank_cd(d.terms,q.terms) AS rank FROM joppa.search_documents d,q
      WHERE d.workspace=ANY($1) AND ($3::text IS NULL OR d.kind=$3) AND d.terms @@ q.terms
      UNION ALL
      SELECT r.workspace,r.kind,r.local_id,r.revision,false,false,ts_rank_cd(r.terms,q.terms) FROM joppa.search_revisions r,q
      WHERE $4 AND r.workspace=ANY($1) AND ($3::text IS NULL OR r.kind=$3) AND r.terms @@ q.terms AND NOT {COVERED}
      UNION ALL
      SELECT d.workspace,d.kind,d.local_id,-1,true,true,0::real FROM joppa.item_ids i LEFT JOIN joppa.object_aliases a ON a.workspace=i.workspace AND a.address=i.kind||':'||i.local_id JOIN joppa.search_documents d
      ON d.workspace=i.workspace AND d.kind=COALESCE(split_part(a.target,':',1),i.kind) AND d.local_id=COALESCE(split_part(a.target,':',2),i.local_id)
      WHERE i.id=$2 AND d.workspace=ANY($1) AND ($3::text IS NULL OR d.kind=$3)
      UNION ALL
      SELECT d.workspace,d.kind,d.local_id,-1,true,false,0::real FROM joppa.search_documents d
      WHERE d.workspace=ANY($1) AND ($3::text IS NULL OR d.kind=$3)
        AND d.kind||':'||d.local_id=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=d.workspace AND address=$2),$2))
      SELECT workspace,kind,local_id,rev,bool_or(exact),bool_or(item) FROM matches GROUP BY workspace,kind,local_id,rev
      ORDER BY bool_or(exact) DESC,max(rank) DESC,workspace,kind,local_id,rev"), &[&scope, &query, &kind, &flags.all_versions]).await?;
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
        // Restrict the candidate set by workspace before exact cosine ranking. Each indexed
        // row has a distance, so only the 100 nearest rows count as vector matches.
        let rows = tx.query(&format!("SELECT workspace,kind,local_id,rev FROM (
            SELECT d.workspace,d.kind,d.local_id,-1 AS rev,d.embedding <=> $4::text::vector AS distance FROM joppa.search_documents d
            WHERE d.workspace=ANY($1) AND ($2::text IS NULL OR d.kind=$2) AND d.embedding IS NOT NULL AND d.embedding_profile=$3
            UNION ALL
            SELECT r.workspace,r.kind,r.local_id,r.revision,r.embedding <=> $4::text::vector FROM joppa.search_revisions r
            WHERE $5 AND r.workspace=ANY($1) AND ($2::text IS NULL OR r.kind=$2) AND r.embedding IS NOT NULL AND r.embedding_profile=$3 AND NOT {COVERED}
          ) c ORDER BY distance,workspace,kind,local_id,rev LIMIT 100"), &[&scope, &kind, &PROFILE, &vector, &flags.all_versions]).await?;
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
    for ws in &scope {
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
    Ok(json!({
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
    }))
}

#[path = "search_excerpt.rs"]
mod excerpt_text;
use excerpt_text::excerpt;
