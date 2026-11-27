//! The candidate queries of a search: the counts of the index, the word and exact matches,
//! and the 100 nearest vectors. Each query keeps only the objects that the node filters keep
//! (src/node_filter.rs), so `total` and the cursor count the same results.
use crate::embeddings::PROFILE;
use crate::node_filter::Bound;
use anyhow::Result;
use tokio_postgres::types::ToSql;
use tokio_postgres::{Row, Transaction};

/// A revision row that the current document already covers is not a candidate.
const COVERED: &str = "EXISTS(SELECT 1 FROM joppa.search_documents c JOIN joppa.objects o ON o.workspace=c.workspace AND o.address=c.kind||':'||c.local_id WHERE c.workspace=r.workspace AND c.kind||':'||c.local_id=r.address AND o.revision=r.revision)";

/// The filter of the rows of one table alias: `$flag` is true when a filter can drop a row,
/// and the clause of the node filters starts at `$first`.
fn keep(alias: &str, flag: usize, bound: &Bound, first: usize) -> String {
    format!(
        "(NOT ${flag}::boolean OR EXISTS(SELECT 1 FROM joppa.objects o WHERE o.workspace={alias}.workspace AND o.address={alias}.kind||':'||{alias}.local_id AND {}))",
        bound.clause(first)
    )
}

fn params<'a>(
    head: Vec<&'a (dyn ToSql + Sync)>,
    narrows: &'a bool,
    bound: &'a Bound,
) -> Vec<&'a (dyn ToSql + Sync)> {
    let mut all = head;
    all.push(narrows);
    all.extend(bound.params());
    all
}

/// The number of indexed and of embedded rows that the filters keep. Without all_versions
/// the statement has no branch on the revisions.
pub(super) async fn counts(
    tx: &Transaction<'_>,
    scope: &[String],
    all_versions: bool,
    bound: &Bound,
) -> Result<(i64, i64)> {
    let (d, r) = (keep("d", 3, bound, 4), keep("r", 3, bound, 4));
    let (indexed, embedded) = if all_versions {
        (
            format!(
                "+ (SELECT count(*) FROM joppa.search_revisions r WHERE workspace=ANY($1) AND {r} AND NOT {COVERED})"
            ),
            format!(
                "+ (SELECT count(*) FROM joppa.search_revisions r WHERE workspace=ANY($1) AND {r} AND embedding IS NOT NULL AND embedding_profile=$2 AND NOT {COVERED})"
            ),
        )
    } else {
        Default::default()
    };
    let row = tx.query_one(&format!("SELECT (SELECT count(*) FROM joppa.search_documents d WHERE workspace=ANY($1) AND {d}) {indexed},
        (SELECT count(*) FROM joppa.search_documents d WHERE workspace=ANY($1) AND {d} AND embedding IS NOT NULL AND embedding_profile=$2) {embedded}"),
        &params(vec![&scope, &PROFILE], &bound.narrows(), bound)).await?;
    Ok((row.get(0), row.get(1)))
}

/// The word matches, the exact item ID and the exact address or local ID, best first. Each
/// row gives workspace, kind, local ID, revision (-1 for the current document), exact and item.
pub(super) async fn words(
    tx: &Transaction<'_>,
    scope: &[String],
    query: &str,
    all_versions: bool,
    bound: &Bound,
) -> Result<Vec<Row>> {
    let (d, r) = (keep("d", 3, bound, 4), keep("r", 3, bound, 4));
    let revisions = if all_versions {
        format!("UNION ALL
      SELECT r.workspace,r.kind,r.local_id,r.revision,false,false,ts_rank_cd(r.terms,q.terms) FROM joppa.search_revisions r,q
      WHERE r.workspace=ANY($1) AND {r} AND r.terms @@ q.terms AND NOT {COVERED}
      UNION ALL
      SELECT r.workspace,r.kind,r.local_id,r.revision,false,false,0::real FROM joppa.search_revisions r
      WHERE r.workspace=ANY($1) AND {r} AND {} AND NOT {COVERED}", title_match("r"))
    } else {
        String::new()
    };
    let titles = title_match("d");
    Ok(tx.query(&format!("WITH q AS (SELECT websearch_to_tsquery('simple',$2) || websearch_to_tsquery('english',$2) || websearch_to_tsquery('russian',$2) AS terms), matches AS (
      SELECT d.workspace,d.kind,d.local_id,-1 AS rev,false AS exact,false AS item,ts_rank_cd(d.terms,q.terms) AS rank FROM joppa.search_documents d,q
      WHERE d.workspace=ANY($1) AND {d} AND d.terms @@ q.terms
      UNION ALL
      SELECT d.workspace,d.kind,d.local_id,-1,false,false,0::real FROM joppa.search_documents d
      WHERE d.workspace=ANY($1) AND {d} AND {titles}
      {revisions}
      UNION ALL
      SELECT d.workspace,d.kind,d.local_id,-1,true,true,0::real FROM joppa.item_ids i LEFT JOIN joppa.object_aliases a ON a.workspace=i.workspace AND a.address=i.kind||':'||i.local_id JOIN joppa.search_documents d
      ON d.workspace=i.workspace AND d.kind=COALESCE(split_part(a.target,':',1),i.kind) AND d.local_id=COALESCE(split_part(a.target,':',2),i.local_id)
      WHERE i.id=joppa.short_item_id($2) AND d.workspace=ANY($1) AND {d}
      UNION ALL
      SELECT d.workspace,d.kind,d.local_id,-1,true,false,0::real FROM joppa.search_documents d
      WHERE d.workspace=ANY($1) AND {d}
        AND (d.kind||':'||d.local_id=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=d.workspace AND address=$2),$2)
          OR ($2 ~ '^[a-z0-9]{{8}}$' AND (d.local_id=$2 OR d.kind||':'||d.local_id IN (SELECT target FROM joppa.object_aliases WHERE workspace=d.workspace AND split_part(address,':',2)=$2)))))
      SELECT workspace,kind,local_id,rev,bool_or(exact),bool_or(item) FROM matches GROUP BY workspace,kind,local_id,rev
      ORDER BY bool_or(exact) DESC,max(rank) DESC,EXISTS(SELECT 1 FROM joppa.search_documents c WHERE c.workspace=matches.workspace AND c.kind=matches.kind AND c.local_id=matches.local_id) DESC,workspace,kind,local_id,rev"),
      &params(vec![&scope, &query], &bound.narrows(), bound)).await?)
}

/// A title of `alias` that holds the whole query, or one of its words with `/`, `:` or `_`,
/// of 3 characters or more. Such a node is a candidate also when other words match nothing.
fn title_match(alias: &str) -> String {
    format!(
        "char_length(trim($2))>=3 AND (strpos(lower({alias}.title),lower(trim($2)))>0 OR EXISTS(SELECT 1 FROM regexp_split_to_table(lower($2),'\\s+') t WHERE char_length(t)>=3 AND t ~ '[/:_]' AND strpos(lower({alias}.title),t)>0))"
    )
}

/// The title of each key: the current document, else the revision.
pub(super) async fn titles(tx: &Transaction<'_>, keys: &str) -> Result<Vec<Row>> {
    Ok(tx.query("SELECT k.workspace,k.kind,k.id,k.rev,COALESCE(d.title,r.title,'') FROM jsonb_to_recordset($1::text::jsonb) AS k(workspace text,kind text,id text,rev int)
      LEFT JOIN joppa.search_documents d ON k.rev<0 AND d.workspace=k.workspace AND d.kind=k.kind AND d.local_id=k.id
      LEFT JOIN joppa.search_revisions r ON k.rev>=0 AND r.workspace=k.workspace AND r.address=k.kind||':'||k.id AND r.revision=k.rev", &[&keys]).await?)
}

/// The score that a title adds for `query`: 0.10 for the same text, 0.035 when it holds the
/// query, 0.025 when it holds each word of 3 characters or more, or a word with `/`, `:` or
/// `_`. Each stays below the 1.0 of an exact address or ID.
pub(super) fn title_boost(title: &str, query: &str) -> f64 {
    let (title, query) = (title.trim().to_lowercase(), query.trim().to_lowercase());
    if query.chars().count() < 3 {
        return 0.0;
    }
    if title == query {
        return 0.10;
    }
    if title.contains(&query) {
        return 0.035;
    }
    let words: Vec<&str> = query
        .split_whitespace()
        .filter(|w| w.chars().count() >= 3)
        .collect();
    let all = !words.is_empty() && words.iter().all(|w| title.contains(w));
    let token = words
        .iter()
        .any(|w| w.contains(['/', ':', '_']) && title.contains(w));
    if all || token { 0.025 } else { 0.0 }
}

/// The 100 nearest vectors by cosine distance, nearest first. The current documents can come
/// from the HNSW index (`search_documents_embedding_hnsw`): its iterative scan continues in
/// distance order until enough rows pass the workspace and node filters, up to 1,000,000 index
/// rows. The inner list holds 200 rows, so the outer sort reads each group of equal distance
/// at row 100 whole and orders it by key. Without all_versions the statement has no branch on
/// the revisions, which have no vector index.
pub(super) async fn nearest(
    tx: &Transaction<'_>,
    scope: &[String],
    vector: &str,
    all_versions: bool,
    bound: &Bound,
) -> Result<Vec<Row>> {
    let (d, r) = (keep("d", 4, bound, 5), keep("r", 4, bound, 5));
    let revisions = if all_versions {
        format!("UNION ALL
        SELECT r.workspace,r.kind,r.local_id,r.revision,r.embedding <=> $3::text::vector FROM joppa.search_revisions r
        WHERE r.workspace=ANY($1) AND {r} AND r.embedding IS NOT NULL AND r.embedding_profile=$2 AND NOT {COVERED}")
    } else {
        String::new()
    };
    tx.batch_execute(
        "SET LOCAL hnsw.ef_search = 100; SET LOCAL hnsw.iterative_scan = strict_order; SET LOCAL hnsw.max_scan_tuples = 1000000",
    )
    .await?;
    Ok(tx.query(&format!("SELECT workspace,kind,local_id,rev FROM (
        (SELECT d.workspace,d.kind,d.local_id,-1 AS rev,d.embedding <=> $3::text::vector AS distance FROM joppa.search_documents d
        WHERE d.workspace=ANY($1) AND {d} AND d.embedding IS NOT NULL AND d.embedding_profile=$2
        ORDER BY d.embedding <=> $3::text::vector LIMIT 200)
        {revisions}
      ) c ORDER BY distance,workspace,kind,local_id,rev LIMIT 100"),
      &params(vec![&scope, &PROFILE, &vector], &bound.narrows(), bound)).await?)
}

#[cfg(test)]
#[path = "search_candidates_tests.rs"]
mod tests;
