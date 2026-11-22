//! History: the journal entries of a workspace, one entry for each recorded change. Each
//! entry is open until a member marks it processed. A planner reads the oldest open entry,
//! does the work and marks it with a reason and the nodes that the work created or changed.
//! A mark adds no journal entry, so History never grows from its own marks.
use super::*;

#[path = "store_history_filters.rs"]
mod filters;
#[path = "store_history_mark.rs"]
mod mark;
use filters::{bind_targets, filters, scope};
pub use mark::mark_history;

/// The largest default History page.
pub const HISTORY_LIMIT: i64 = 25;
/// The largest page of History.
pub const HISTORY_MAX_LIMIT: i64 = 100;

/// Read one page of History. One read transaction gives the page, its items and the current
/// status of each object.
pub async fn read_history(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    args: &Value,
) -> Result<Value> {
    read(pool, workspace, actor, args, true).await
}

/// The MCP History read: the same page without the current status of each object, so it reads
/// no object signal (ac:pfewsd6m).
pub async fn read_history_lean(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    args: &Value,
) -> Result<Value> {
    read(pool, workspace, actor, args, false).await
}

async fn read(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    args: &Value,
    with_signals: bool,
) -> Result<Value> {
    let s = scope(args)?;
    let mut conn = pool.get().await?;
    let id = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &id, actor, false).await?;
    let conn = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let (public, position) = super::head(&*conn, &id).await?;
    let (target, under) = bind_targets(&*conn, &id, &s).await?;
    // Oldest first continues after the cursor. Newest first continues before it.
    let (low, high) = match (s.cursor, s.newest) {
        (Some(p), false) => (s.after.max(p), i64::MAX),
        (Some(p), true) => (s.after, p - 1),
        (None, _) => (s.after, i64::MAX),
    };
    let order = if s.newest { "DESC" } else { "ASC" };
    let rows = conn
        .query(
            &format!(
                "SELECT e.position, e.recorded_at::text, e.actor, e.command::text, m.actor, m.marked_at::text, m.reason, m.nodes::text, e.alias, m.alias AS mark_alias FROM joppa.events e LEFT JOIN joppa.history_marks m ON m.workspace=e.workspace AND m.position=e.position WHERE e.workspace=$1 AND e.position>$2 AND e.position<=$3 AND (NOT $4 OR m.position IS NULL) AND {} ORDER BY e.position {order} LIMIT $5",
                filters(6)
            ),
            &[
                &id,
                &low,
                &high,
                &s.open,
                &(s.limit + 1),
                &s.alias,
                &s.op,
                &s.kind,
                &target,
                &under,
            ],
        )
        .await?;
    let next_item: Option<i64> = conn
        .query_one(
            "SELECT min(e.position) FROM joppa.events e WHERE e.workspace=$1 AND NOT EXISTS (SELECT 1 FROM joppa.history_marks m WHERE m.workspace=e.workspace AND m.position=e.position)",
            &[&id],
        )
        .await?
        .get(0);
    // `total` counts every entry of the read after `after`, also the entries before the cursor.
    let total: i64 = conn
        .query_one(
            &format!(
                "SELECT count(*) FROM joppa.events e LEFT JOIN joppa.history_marks m ON m.workspace=e.workspace AND m.position=e.position WHERE e.workspace=$1 AND e.position>$2 AND (NOT $3 OR m.position IS NULL) AND {}",
                filters(4)
            ),
            &[
                &id,
                &s.after,
                &s.open,
                &s.alias,
                &s.op,
                &s.kind,
                &target,
                &under,
            ],
        )
        .await?
        .get(0);
    let more = rows.len() as i64 > s.limit;
    let rows = &rows[..rows.len().min(s.limit as usize)];
    let mut items = super::history_items::items(&*conn, &id, rows).await?;
    // The current status of each object, with the stored rollup as on the board.
    let addresses: Vec<String> = items
        .iter()
        .filter_map(|item| item["address"].as_str().map(str::to_owned))
        .collect();
    let mut signals = std::collections::BTreeMap::<String, Value>::new();
    if with_signals {
        signals = conn
            .query(
                "SELECT address, (COALESCE(signal,'{}'::jsonb)||COALESCE(rollup->'signal','{}'::jsonb))::text FROM joppa.objects WHERE workspace=$1 AND address=ANY($2)",
                &[&id, &addresses],
            )
            .await?
            .iter()
            .map(|row| {
                let signal = serde_json::from_str(row.get(1))?;
                Ok((row.get(0), signal))
            })
            .collect::<Result<_>>()?;
    }
    conn.commit().await?;
    for (item, row) in items.iter_mut().zip(rows) {
        if with_signals {
            let signal = item["address"].as_str().and_then(|a| signals.get(a));
            item["signal"] = signal.cloned().unwrap_or(Value::Null);
        }
        match row.get::<_, Option<String>>(4) {
            Some(by) => {
                item["status"] = json!("processed");
                item["processed"] = json!({
                    "actor": by,
                    "at": row.get::<_, Option<String>>(5),
                    "reason": row.get::<_, Option<String>>(6),
                    "nodes": serde_json::from_str::<Value>(&row.get::<_, Option<String>>(7).unwrap_or_else(|| "[]".into()))?,
                });
                crate::actor_alias::put(&mut item["processed"], row.get(9));
            }
            None => item["status"] = json!("open"),
        }
    }
    let next_cursor = (more && !rows.is_empty())
        .then(|| format!("h{}", rows.last().map(|r| r.get::<_, i64>(0)).unwrap_or(0)));
    Ok(json!({
        "workspace": public,
        "position": position,
        "items": items,
        "total": total,
        "next_item": next_item,
        "next_cursor": next_cursor,
        "open": s.open,
        "order": if s.newest { "newest" } else { "oldest" },
        "after": s.after,
        "limit": s.limit,
    }))
}

#[cfg(test)]
use filters::page_limit;
#[cfg(test)]
use mark::parse_mark;

#[cfg(test)]
#[path = "store_history_tests.rs"]
mod tests;
