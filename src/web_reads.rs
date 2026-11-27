//! Read routes: the board, objects, History and the tree.
use super::*;
#[path = "web_counts.rs"]
mod counts;
#[path = "web_reads_many.rs"]
mod many;
#[path = "web_reads_query.rs"]
mod query;
pub(super) use many::history;
pub(super) use query::ObjectQuery;

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
pub(super) async fn objects(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<ObjectQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    let order = match q.list_order() {
        Ok(order) => order,
        Err(e) => return result(Err(e)),
    };
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
    // A list with the fields blocked or dependents reads the dependency facts.
    let dependency = store::CardNeeds {
        dependency: joppa::read_compact::list_needs(fields.as_deref()).dependency,
        ..Default::default()
    };
    result(
        store::read_objects_with(
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
                order,
            },
            dependency,
        )
        .await
        .map(|reply| derived_one(reply, q.address.is_some(), fields.as_deref()))
        .map(merged)
        .map(|reply| joppa::read_shapes::keep_item_fields(reply, fields.as_deref())),
    )
}

/// A read of one address gives the derived fields that `fields` names, such as excerpt.
fn derived_one(mut reply: Value, one: bool, fields: Option<&[String]>) -> Value {
    let Some(fields) = fields.filter(|_| one) else {
        return reply;
    };
    let named = joppa::read_compact::object_fields(&reply, None, fields);
    for field in fields
        .iter()
        .filter(|f| joppa::read_compact::DERIVED.contains(&f.as_str()))
    {
        reply[field.as_str()] = named[field.as_str()].clone();
    }
    reply
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

/// An object read over HTTP keeps the computed values in its data, as the board reads them.
/// MCP gives them apart, in `computed`.
fn merged(mut reply: Value) -> Value {
    let computed = reply.as_object_mut().and_then(|m| m.remove("computed"));
    let data = reply.get_mut("document").and_then(|d| d.get_mut("data"));
    if let (Some(Value::Object(computed)), Some(data)) =
        (computed, data.and_then(Value::as_object_mut))
    {
        data.extend(computed);
    }
    reply
}
