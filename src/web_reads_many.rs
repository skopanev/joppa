//! Part of `reads`: the object read of up to 25 addresses in one request, separated by
//! commas, as the address read of joppa_objects. Flow view reads the facts of its steps with
//! it. The reply gives `items` in the order of the request, each the list item of one object:
//! address, kind, title, signal, item_id and present. `fields` keeps the named keys.
use super::*;

/// The most addresses of one read: one page.
const MOST: usize = 25;

/// The read of the address list `list`. It takes no other key than `fields`.
pub(super) async fn read(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    list: &str,
    other: &[&str],
    fields: Option<&[String]>,
) -> Result<Value> {
    ensure!(
        other.iter().all(|key| *key == "address"),
        "an address list takes only fields"
    );
    let addresses: Vec<&str> = list.split(',').map(str::trim).collect();
    ensure!(
        addresses.iter().all(|a| !a.is_empty()),
        "empty address in the address list"
    );
    ensure!(
        addresses.len() <= MOST,
        "at most {MOST} addresses in one call"
    );
    let mut items = Vec::with_capacity(addresses.len());
    for address in addresses {
        let scope = store::ObjectScope {
            address: Some(address),
            ..Default::default()
        };
        let reply = store::read_objects(pool, workspace, actor, &scope).await?;
        let document = &reply["document"];
        let mut item = json!({"address": reply["address"], "kind": reply["kind"],
            "title": document["data"]["title"], "signal": reply["signal"],
            "item_id": reply["item_id"], "present": document["present"] != false});
        if let Some(fields) = fields {
            joppa::read_shapes::keep_fields(&mut item, fields);
        }
        items.push(item);
    }
    Ok(json!({"items": items}))
}

#[derive(Deserialize)]
pub(in crate::web) struct HistoryQuery {
    limit: Option<i64>,
    cursor: Option<String>,
    open: Option<bool>,
    order: Option<String>,
    after: Option<i64>,
    by_alias: Option<String>,
    op: Option<String>,
    kind: Option<String>,
    target: Option<String>,
    under: Option<String>,
}

/// History: one entry for each journal event, open until a member marks it processed.
pub(in crate::web) async fn history(
    State(app): State<App>,
    Path(workspace): Path<String>,
    Query(q): Query<HistoryQuery>,
    headers: HeaderMap,
) -> ApiResult {
    let actor = authenticate(&app, &headers).await?;
    let limit = q.limit.unwrap_or(store::HISTORY_LIMIT);
    let args = serde_json::json!({
        "limit": limit, "cursor": q.cursor, "open": q.open, "order": q.order, "after": q.after,
        "by_alias": q.by_alias, "op": q.op, "kind": q.kind, "target": q.target, "under": q.under,
    });
    result(store::read_history(&app.pool, &workspace, &actor, &args).await)
}
