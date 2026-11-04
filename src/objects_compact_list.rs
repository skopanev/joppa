//! The lean list of requirement:read-contract: the present objects that the node filters
//! keep, or the roots without a filter, in address order. A `status` with the archived tone
//! also keeps the current archived objects. Each row gives only the address,
//! the title and the status tone, with no item ID, process or priority lookup. `total`
//! counts every matching object. The reply gives the workspace and the journal position of
//! its snapshot. The tone is the one that the last write stored in the row: the rollup
//! tone, else the signal tone. One query reads it, so a status rule shows here too.
use super::*;

pub async fn compact_list(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    filter: &crate::node_filter::NodeFilter,
    limit: usize,
    cursor: Option<&str>,
) -> Result<Value> {
    ensure!(
        cursor.is_none_or(|c| c
            .split_once(':')
            .is_some_and(|(kind, id)| !kind.is_empty() && !id.is_empty() && !id.contains(':'))),
        "invalid object cursor; start again without cursor"
    );
    let (mut conn, workspace) = authorized(pool, workspace, actor).await?;
    // One snapshot: the journal position belongs to the rows of this page.
    let conn = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let head = conn
        .query_one(
            "SELECT position,COALESCE(state->>'workspace_id',workspace) FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    // The stored rollup belongs to the last write only when the projection is ready.
    crate::store::objects::projection::ensure_ready(&*conn, &workspace).await?;
    // A status list with the archived tone also keeps the current archived nodes.
    let bound =
        crate::node_filter::bind(&*conn, &workspace, filter, true, filter.archived()).await?;
    let query = format!(
        "SELECT x.address,x.title,x.tone,x.total
        FROM (SELECT o.address,o.title,{tone} AS tone,count(*) OVER() AS total FROM joppa.objects o
            WHERE o.workspace=$1 AND (o.present OR $13) AND {clause}) x
        WHERE x.address>$11 ORDER BY x.address LIMIT $12",
        tone = crate::node_filter::TONE,
        clause = bound.clause(2),
    );
    let cursor = cursor.unwrap_or("");
    let limit = (limit + 1) as i64;
    let archived = filter.archived();
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = vec![&workspace];
    params.extend(bound.params());
    params.extend([
        &cursor as &(dyn tokio_postgres::types::ToSql + Sync),
        &limit,
        &archived,
    ]);
    let rows = conn.query(&query, &params).await?;
    let limit = (limit - 1) as usize;
    let total: i64 = rows.first().map_or(0, |r| r.get(3));
    let mut items: Vec<Value> = rows
        .iter()
        .take(limit)
        .map(|r| {
            let mut item =
                json!({"address": r.get::<_, String>(0), "title": r.get::<_, String>(1)});
            if let Some(tone) = r.get::<_, Option<String>>(2).filter(|t| !t.is_empty()) {
                item["status"] = json!(tone);
            }
            item
        })
        .collect();
    let mut reply = json!({"workspace": head.get::<_, String>(1), "position": head.get::<_, i64>(0), "total": total});
    if rows.len() > limit {
        reply["next_cursor"] = items.last().map_or(Value::Null, |i| i["address"].clone());
    }
    reply["items"] = Value::Array(std::mem::take(&mut items));
    Ok(reply)
}
