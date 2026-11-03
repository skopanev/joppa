//! The search route of the board: the same hybrid search as the MCP tool joppa_search, for
//! the same members, with the flags all_versions and all_workspaces, the node filters and
//! the cursor of the next page.
use super::*;

#[derive(Deserialize)]
pub(super) struct SearchQuery {
    q: Option<String>,
    kind: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
    all_versions: Option<bool>,
    all_workspaces: Option<bool>,
    parent: Option<String>,
    depth: Option<String>,
    status: Option<String>,
    owner: Option<String>,
    assignee: Option<String>,
    ready: Option<String>,
    data: Option<String>,
}

pub(super) async fn search(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<SearchQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    // The node filters, in the same form as the objects route.
    let filter = match joppa::node_filter::NodeFilter::parse_query(&[
        ("parent", q.parent.as_deref()),
        ("depth", q.depth.as_deref()),
        ("kind", q.kind.as_deref()),
        ("status", q.status.as_deref()),
        ("owner", q.owner.as_deref()),
        ("assignee", q.assignee.as_deref()),
        ("ready", q.ready.as_deref()),
        ("data", q.data.as_deref()),
    ]) {
        Ok(filter) => filter,
        Err(e) => return result(Err(e)),
    };
    let offset = match q.cursor.as_deref().map(str::parse) {
        None => 0,
        Some(Ok(offset)) => offset,
        Some(Err(_)) => return result(Err(anyhow::anyhow!("invalid cursor"))),
    };
    result(
        store::search::find_with(
            &app.pool,
            &actor,
            &workspace,
            q.q.as_deref().unwrap_or(""),
            &filter,
            q.limit.unwrap_or(50),
            app.embedder.as_ref(),
            store::search::Flags {
                all_versions: q.all_versions.unwrap_or(false),
                all_workspaces: q.all_workspaces.unwrap_or(false),
                offset,
            },
        )
        .await,
    )
}
