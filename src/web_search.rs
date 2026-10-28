//! The search route of the board: the same hybrid search as the MCP tool joppa_search, for
//! the same members, with the flags all_versions and all_workspaces.
use super::*;

#[derive(Deserialize)]
pub(super) struct SearchQuery {
    q: Option<String>,
    kind: Option<String>,
    limit: Option<i64>,
    all_versions: Option<bool>,
    all_workspaces: Option<bool>,
}

pub(super) async fn search(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<SearchQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(
        store::search::find_with(
            &app.pool,
            &actor,
            &workspace,
            q.q.as_deref().unwrap_or(""),
            q.kind.as_deref(),
            q.limit.unwrap_or(50),
            app.embedder.as_ref(),
            store::search::Flags {
                all_versions: q.all_versions.unwrap_or(false),
                all_workspaces: q.all_workspaces.unwrap_or(false),
            },
        )
        .await,
    )
}
