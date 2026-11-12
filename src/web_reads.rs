//! Read routes: the board, objects, History and the tree.
use super::*;
#[path = "web_counts.rs"]
mod counts;
#[path = "web_reads_many.rs"]
mod many;

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
pub(super) async fn board(
    State(app): State<App>,
    Path(workspace): Path<String>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    result(store::board_read(&app.pool, &workspace, &actor).await)
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
    /// Refused: work_filter was removed.
    work_filter: Option<String>,
    /// Refused: links_of was removed.
    links_of: Option<String>,
    /// The address whose edges to read in pages of 25, with optional direction, relation
    /// and kind.
    relations_of: Option<String>,
    direction: Option<String>,
    relation: Option<String>,
    /// The relation key whose members to read in pages of 25 (src/relation_members.rs).
    in_relation: Option<String>,
    /// Comma-separated keys to keep in each list item; the address always stays.
    fields: Option<String>,
    depth: Option<String>,
    status: Option<String>,
    owner: Option<String>,
    assignee: Option<String>,
    /// Refused: the ready filter was removed.
    ready: Option<String>,
    data: Option<String>,
    /// true gives only the total of the list (src/node_counts.rs).
    count: Option<String>,
    /// 1 to 3 comma-separated keys of the groups of the list.
    group_by: Option<String>,
}
impl ObjectQuery {
    /// The node filters, in the same form as joppa_objects.
    fn filter(&self) -> anyhow::Result<joppa::node_filter::NodeFilter> {
        joppa::node_filter::NodeFilter::parse_query(&[
            ("parent", self.parent.as_deref()),
            ("depth", self.depth.as_deref()),
            ("kind", self.kind.as_deref()),
            ("status", self.status.as_deref()),
            ("owner", self.owner.as_deref()),
            ("assignee", self.assignee.as_deref()),
            ("ready", self.ready.as_deref()),
            ("data", self.data.as_deref()),
        ])
    }
    /// The names of the keys that the request sends, outside fields, limit and cursor.
    fn other_keys(&self) -> Vec<&'static str> {
        let texts = [
            ("address", &self.address),
            ("parent", &self.parent),
            ("kind", &self.kind),
            ("note_status", &self.note_status),
            ("related_to", &self.related_to),
            ("work_filter", &self.work_filter),
            ("links_of", &self.links_of),
            ("relations_of", &self.relations_of),
            ("direction", &self.direction),
            ("relation", &self.relation),
            ("depth", &self.depth),
            ("status", &self.status),
            ("owner", &self.owner),
            ("assignee", &self.assignee),
            ("ready", &self.ready),
            ("data", &self.data),
        ];
        let mut keys: Vec<&'static str> = texts
            .into_iter()
            .filter_map(|(key, value)| value.is_some().then_some(key))
            .collect();
        keys.extend(self.revision.map(|_| "revision"));
        keys.extend(self.history.map(|_| "history"));
        keys
    }
}
pub(super) async fn objects(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<ObjectQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    if q.links_of.is_some() {
        return result(Err(anyhow::anyhow!(joppa::removed_ops::LINKS_OF)));
    }
    if q.ready.is_some() {
        return result(Err(anyhow::anyhow!(joppa::removed_ops::READY_FILTER)));
    }
    if q.count.is_some() || q.group_by.is_some() {
        return result(counts::read(&app.pool, &workspace, &actor, &q).await);
    }
    if let Some(address) = q.relations_of.as_deref() {
        return result(
            store::link_page(
                &app.pool,
                &workspace,
                &actor,
                address,
                q.direction.as_deref(),
                q.relation.as_deref(),
                q.kind.as_deref(),
                q.limit.unwrap_or(25).min(25),
                q.cursor.as_deref(),
            )
            .await,
        );
    }
    if q.work_filter.is_some() {
        return result(Err(anyhow::anyhow!(joppa::removed_ops::WORK_FILTER)));
    }
    let fields: Option<Vec<String>> = q.fields.as_deref().map(|f| {
        f.split(',')
            .filter(|k| !k.is_empty())
            .map(str::to_owned)
            .collect()
    });
    if let Err(e) = joppa::read_shapes::check_removed(fields.as_deref().unwrap_or_default()) {
        return result(Err(e));
    }
    if let Some(list) = q.address.as_deref().filter(|a| a.contains(',')) {
        let (other, fields) = (q.other_keys(), fields.as_deref());
        return result(many::read(&app.pool, &workspace, &actor, list, &other, fields).await);
    }
    let data = fields
        .as_deref()
        .is_some_and(joppa::read_compact::needs_data);
    if let Some(key) = q.in_relation.as_deref() {
        if let Err(e) = joppa::relation_members::check_keys(q.other_keys()) {
            return result(Err(e));
        }
        let limit = joppa::relation_members::page(q.limit);
        let read = store::relation_members(
            &app.pool,
            &workspace,
            &actor,
            key,
            (limit, q.cursor.as_deref(), data, &[]),
        );
        return result(
            read.await
                .map(|reply| joppa::read_shapes::keep_item_fields(reply, fields.as_deref())),
        );
    }
    let filter = match q.filter() {
        Ok(filter) => filter,
        Err(e) => return result(Err(e)),
    };
    result(
        store::read_objects(
            &app.pool,
            &workspace,
            &actor,
            &store::ObjectScope {
                address: q.address.as_deref(),
                filter,
                revision: q.revision,
                history: q.history.unwrap_or(false),
                limit: q.limit.unwrap_or(50),
                cursor: q.cursor.as_deref(),
                note_status: q.note_status.as_deref(),
                related_to: q.related_to.as_deref(),
                data,
            },
        )
        .await
        .map(|reply| joppa::read_shapes::keep_item_fields(reply, fields.as_deref())),
    )
}
#[derive(Deserialize)]
pub(super) struct HistoryQuery {
    limit: Option<i64>,
    cursor: Option<String>,
    open: Option<bool>,
    order: Option<String>,
    after: Option<i64>,
}
/// History: one entry for each journal event, open until a member marks it processed.
pub(super) async fn history(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<HistoryQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    let limit = q.limit.unwrap_or(store::HISTORY_LIMIT);
    let args = serde_json::json!({"limit": limit, "cursor": q.cursor, "open": q.open, "order": q.order, "after": q.after});
    result(store::read_history(&app.pool, &workspace, &actor, &args).await)
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
