//! The search route of the board: the same full-text and semantic search as the MCP tool
//! joppa_search, for the same members.
use super::*;

#[derive(Deserialize)]
pub(super) struct SearchQuery {
    q: Option<String>,
    kind: Option<String>,
    limit: Option<i64>,
}

pub(super) async fn search(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<SearchQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(
        store::search::find(
            &app.pool,
            &actor,
            &workspace,
            q.q.as_deref().unwrap_or(""),
            q.kind.as_deref(),
            q.limit.unwrap_or(50),
            app.embedder.as_ref(),
        )
        .await,
    )
}
