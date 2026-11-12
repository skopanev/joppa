//! The search query. Full-text and vector candidates come from the current documents of the
//! workspaces in scope, and with all_versions also from each object revision. Reciprocal
//! rank with the constant 60 fuses them. An exact item ID, address or generated 8-character
//! local ID gives only its matches, and a match of that form sends no provider request.
//! A candidate key is (workspace, kind, id, revision), where revision -1 is the current
//! document. A revision row that the current document already covers is not a candidate.
use super::*;
use crate::embeddings::{self, Embedder};
use std::collections::BTreeMap;

/// The scope of a search. Both flags are false by default. `offset` skips that many ranked
/// results, so a cursor continues the same ranking. `lean` is the MCP read (tac:s3ydcg5l): it
/// counts no index rows, cuts each excerpt at 200 characters and gives the status tone.
#[derive(Clone, Copy, Debug, Default)]
pub struct Flags {
    pub all_versions: bool,
    pub all_workspaces: bool,
    pub offset: usize,
    pub lean: bool,
}

pub async fn find(
    pool: &Pool,
    actor: &str,
    workspace: &str,
    query: &str,
    kind: Option<&str>,
    limit: i64,
    embedder: Option<&Embedder>,
) -> Result<Value> {
    let filter = crate::node_filter::NodeFilter {
        kinds: kind.map(|k| vec![k.to_owned()]),
        ..Default::default()
    };
    find_with(
        pool,
        actor,
        workspace,
        query,
        &filter,
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
    filter: &crate::node_filter::NodeFilter,
    limit: i64,
    embedder: Option<&Embedder>,
    flags: Flags,
) -> Result<Value> {
    ensure!(
        !query.trim().is_empty() && query.chars().count() <= 2000,
        "query must contain 1–2000 characters"
    );
    ensure!((1..=50).contains(&limit), "limit must be 1–50");
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
    // A query in the form of an ID or an address reads its exact matches first: a match
    // answers with no provider request.
    let words = (query, filter, limit, flags);
    if exact_form(query)
        && let Some(reply) = answer(pool, actor, &scope, words, None, "exact_match").await?
    {
        return Ok(reply);
    }
    let (vector, semantic) = match embedder {
        Some(embedder) => match embedder.embed(&[format!("Instruct: Given a product question, retrieve product requirements and acceptance criteria that answer it.\nQuery: {query}")]).await {
            Ok(v) => (Some(embeddings::literal(&v[0])?), "ready"),
            Err(_) => (None, "unavailable"),
        },
        None => (None, "not_configured"),
    };
    let reply = answer(pool, actor, &scope, words, vector, semantic).await?;
    reply.context("a search without the exact form always answers")
}

/// The query may be an address, a generated 8-character local ID or a public item ID.
fn exact_form(query: &str) -> bool {
    let local = query.len() == 8
        && query
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    !query.contains(char::is_whitespace) && (local || query.contains([':', '-']))
}

#[path = "search_answer.rs"]
mod reply;
use reply::answer;
#[path = "search_candidates.rs"]
mod candidates;
#[path = "search_excerpt.rs"]
mod excerpt_text;
use excerpt_text::{EXCERPT, LEAN_EXCERPT, excerpt};
