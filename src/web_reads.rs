//! Read routes: the board, objects, the timeline, Tasks and the tree.
use super::*;

pub(super) async fn read(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    let scope = store::Scope {
        full: false,
        req: None,
        revision: None,
        history: false,
        target: None,
    };
    result(store::read(&app.pool, &workspace, &actor, &scope).await)
}
#[derive(Deserialize)]
pub(super) struct BoardQuery {
    level: Option<String>,
    id: Option<String>,
    revision: Option<u32>,
}
pub(super) async fn board(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(query): Query<BoardQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(
        store::board_read(
            &app.pool,
            &workspace,
            &actor,
            query.level.as_deref(),
            query.id.as_deref(),
            query.revision,
        )
        .await,
    )
}
#[derive(Deserialize)]
pub(super) struct ObjectQuery {
    address: Option<String>,
    parent: Option<String>,
    kind: Option<String>,
    revision: Option<u32>,
    history: Option<bool>,
    limit: Option<usize>,
    cursor: Option<String>,
    note_status: Option<String>,
    related_to: Option<String>,
    work_filter: Option<String>,
    /// The address whose links of one group to read, with direction and relation.
    links_of: Option<String>,
    direction: Option<String>,
    relation: Option<String>,
    /// Comma-separated keys to keep in each list item; the address always stays.
    fields: Option<String>,
}
pub(super) async fn objects(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<ObjectQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    if let Some(address) = q.links_of.as_deref() {
        return result(
            store::link_page(
                &app.pool,
                &workspace,
                &actor,
                address,
                q.direction.as_deref().unwrap_or(""),
                q.relation.as_deref().unwrap_or(""),
                q.kind.as_deref().unwrap_or(""),
                q.limit.unwrap_or(50),
                q.cursor.as_deref(),
            )
            .await,
        );
    }
    result(
        store::read_objects(
            &app.pool,
            &workspace,
            &actor,
            &store::ObjectScope {
                address: q.address.as_deref(),
                parent: q.parent.as_deref(),
                kind: q.kind.as_deref(),
                revision: q.revision,
                history: q.history.unwrap_or(false),
                limit: q.limit.unwrap_or(50),
                cursor: q.cursor.as_deref(),
                note_status: q.note_status.as_deref(),
                related_to: q.related_to.as_deref(),
                work_filter: q.work_filter.as_deref(),
                data: false,
            },
        )
        .await
        .map(|reply| {
            let fields: Option<Vec<String>> = q.fields.as_deref().map(|f| {
                f.split(',')
                    .filter(|k| !k.is_empty())
                    .map(str::to_owned)
                    .collect()
            });
            joppa::read_shapes::keep_item_fields(reply, fields.as_deref())
        }),
    )
}
#[derive(Deserialize)]
pub(super) struct TimelineQuery {
    after: Option<u64>,
    limit: Option<usize>,
    cursor: Option<String>,
    order: Option<String>,
}
/// The workspace timeline, with the access rules of the other reads.
pub(super) async fn timeline(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<TimelineQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(
        store::read_timeline(
            &app.pool,
            &workspace,
            &actor,
            &store::TimelineScope {
                after: q.after.unwrap_or(0),
                limit: q.limit.unwrap_or(50),
                cursor: q.cursor.as_deref(),
                order: q.order.as_deref(),
            },
        )
        .await,
    )
}
#[derive(Deserialize)]
pub(super) struct HistoryQuery {
    limit: Option<i64>,
    cursor: Option<String>,
    all: Option<bool>,
}
/// History: journal entries that are open until a member marks them processed.
pub(super) async fn history(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<HistoryQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    // The board reads full pages; the MCP tool reads one entry by default.
    let limit = q.limit.unwrap_or(store::HISTORY_LIMIT);
    let args = serde_json::json!({"limit": limit, "cursor": q.cursor, "all": q.all});
    result(store::read_history(&app.pool, &workspace, &actor, &args).await)
}
#[derive(Deserialize)]
pub(super) struct TaskQuery {
    view: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
    project: Option<String>,
    purpose: Option<String>,
    priorities: Option<String>,
}
pub(super) async fn tasks(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(query): Query<TaskQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    let priorities = query
        .priorities
        .as_deref()
        .map(|text| text.split(',').map(str::to_owned).collect::<Vec<_>>())
        .unwrap_or_default();
    result(
        store::task_queue_filtered(
            &app.pool,
            &workspace,
            &actor,
            query.view.as_deref().unwrap_or("available"),
            query.limit.unwrap_or(50),
            query.cursor.as_deref(),
            query.project.as_deref(),
            query.purpose.as_deref(),
            &priorities,
        )
        .await,
    )
}
pub(super) async fn tree_schema(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<ObjectQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::tree_schema(&app.pool, &workspace, &actor, q.revision).await)
}
