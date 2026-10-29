//! History: the journal entries of a workspace, each open until a member marks it
//! processed. A planner reads the oldest open entry, does the work and marks it with
//! a reason and the nodes that the work created or changed. A mark adds no journal
//! entry, so History never grows from its own marks.
use super::*;
use serde::Deserialize;

/// The largest History page.
pub const HISTORY_LIMIT: i64 = 25;
/// The largest node list of one mark.
pub const MARK_NODES_MAX: usize = 15;

/// A cursor holds the last position of the page that it continues.
fn cursor_position(cursor: &str) -> Result<i64> {
    cursor
        .strip_prefix('h')
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| commands::ClientField("invalid history cursor").into())
}

fn page_limit(args: &Value) -> Result<i64> {
    match args.get("limit") {
        None | Some(Value::Null) => Ok(HISTORY_LIMIT),
        Some(value) => value
            .as_i64()
            .filter(|n| (1..=HISTORY_LIMIT).contains(n))
            .ok_or_else(|| commands::ClientField("limit must be between 1 and 25").into()),
    }
}

/// Read open entries oldest first, or every entry with all=true. One read transaction
/// gives the page, its items and the current status of each object.
pub async fn read_history(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    args: &Value,
) -> Result<Value> {
    let limit = page_limit(args)?;
    let all = match args.get("all") {
        None | Some(Value::Null) => false,
        Some(value) => value
            .as_bool()
            .ok_or(commands::ClientField("all must be true or false"))?,
    };
    let after = match args.get("cursor").and_then(Value::as_str) {
        Some(cursor) => cursor_position(cursor)?,
        None => 0,
    };
    let mut conn = pool.get().await?;
    let id = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &id, actor, false).await?;
    let conn = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let rows = conn.query(
        "SELECT e.position, e.recorded_at::text, e.actor, e.command::text, m.actor, m.marked_at::text, m.reason, m.nodes::text FROM joppa.events e LEFT JOIN joppa.history_marks m ON m.workspace=e.workspace AND m.position=e.position WHERE e.workspace=$1 AND e.position>$2 AND ($3 OR m.position IS NULL) ORDER BY e.position LIMIT $4",
        &[&id, &after, &all, &(limit + 1)],
    ).await?;
    let next_item: Option<i64> = conn.query_one(
        "SELECT min(e.position) FROM joppa.events e WHERE e.workspace=$1 AND NOT EXISTS (SELECT 1 FROM joppa.history_marks m WHERE m.workspace=e.workspace AND m.position=e.position)",
        &[&id],
    ).await?.get(0);
    // `total` counts every entry of the view, also the entries before the cursor.
    let total: i64 = conn.query_one(
        "SELECT count(*) FROM joppa.events e LEFT JOIN joppa.history_marks m ON m.workspace=e.workspace AND m.position=e.position WHERE e.workspace=$1 AND ($2 OR m.position IS NULL)",
        &[&id, &all],
    ).await?.get(0);
    let more = rows.len() as i64 > limit;
    let rows = &rows[..rows.len().min(limit as usize)];
    let mut items = super::timeline_items::items(&*conn, &id, rows).await?;
    // The current status of each object, with the stored rollup as on the board.
    let addresses: Vec<String> = items
        .iter()
        .filter_map(|item| item["address"].as_str().map(str::to_owned))
        .collect();
    let signals: std::collections::BTreeMap<String, Value> = conn
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
    conn.commit().await?;
    for (item, row) in items.iter_mut().zip(rows) {
        let signal = item["address"]
            .as_str()
            .and_then(|a| signals.get(a))
            .cloned();
        item["signal"] = signal.unwrap_or(Value::Null);
        match row.get::<_, Option<String>>(4) {
            Some(by) => {
                item["status"] = json!("processed");
                item["processed"] = json!({
                    "actor": by,
                    "at": row.get::<_, Option<String>>(5),
                    "reason": row.get::<_, Option<String>>(6),
                    "nodes": serde_json::from_str::<Value>(&row.get::<_, Option<String>>(7).unwrap_or_else(|| "[]".into()))?,
                });
            }
            None => item["status"] = json!("open"),
        }
    }
    let next_cursor = (more && !rows.is_empty())
        .then(|| format!("h{}", rows.last().map(|r| r.get::<_, i64>(0)).unwrap_or(0)));
    Ok(
        json!({"items": items, "total": total, "next_item": next_item, "next_cursor": next_cursor, "all": all, "limit": limit}),
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mark {
    #[serde(default)]
    workspace: Option<String>,
    position: i64,
    reason: String,
    #[serde(default)]
    nodes: Vec<String>,
}

/// Check a mark before any read of the workspace.
fn parse_mark(args: &Value) -> Result<Mark> {
    for field in ["actor", "at"] {
        if args.get(field).is_some() {
            return Err(commands::ClientField(if field == "actor" {
                "actor comes from the server; do not supply it"
            } else {
                "at comes from the server; do not supply it"
            })
            .into());
        }
    }
    let mark: Mark = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("invalid history mark: {e}"))?;
    let _ = &mark.workspace;
    ensure!(mark.position >= 1, "position must be a journal position");
    ensure!(!mark.reason.trim().is_empty(), "a mark needs a reason");
    ensure!(
        mark.nodes.len() <= MARK_NODES_MAX,
        "a mark names at most 15 nodes"
    );
    let unique: std::collections::BTreeSet<&String> = mark.nodes.iter().collect();
    ensure!(
        unique.len() == mark.nodes.len(),
        "a mark names each node once"
    );
    Ok(mark)
}

/// Mark one open entry processed. The server sets the actor and the time.
pub async fn mark_history(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    args: &Value,
) -> Result<Value> {
    let mark = parse_mark(args)?;
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let id = resolve_workspace(&*tx, workspace).await?;
    access::check(&*tx, &id, actor, false).await?;
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&id],
    )
    .await?;
    let exists: bool = tx
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM joppa.events WHERE workspace=$1 AND position=$2)",
            &[&id, &mark.position],
        )
        .await?
        .get(0);
    ensure!(exists, "position {} is not a journal entry", mark.position);
    let found: i64 = tx
        .query_one(
            "SELECT count(*) FROM unnest($2::text[]) x(address) WHERE EXISTS (SELECT 1 FROM joppa.objects o WHERE o.workspace=$1 AND o.address=COALESCE((SELECT a.target FROM joppa.object_aliases a WHERE a.workspace=$1 AND a.address=x.address), x.address))",
            &[&id, &mark.nodes],
        )
        .await?
        .get(0);
    ensure!(
        found as usize == mark.nodes.len(),
        "every node must be an object of the workspace"
    );
    let nodes = serde_json::to_string(&mark.nodes)?;
    let row = tx.query_opt(
        "INSERT INTO joppa.history_marks(workspace,position,actor,reason,nodes) VALUES($1,$2,$3,$4,$5::text::jsonb) ON CONFLICT (workspace,position) DO NOTHING RETURNING marked_at::text",
        &[&id, &mark.position, &actor, &mark.reason, &nodes],
    ).await?;
    let Some(row) = row else {
        return Err(commands::ClientField("the entry is already processed").into());
    };
    tx.commit().await?;
    Ok(
        json!({"position": mark.position, "status": "processed", "actor": actor, "at": row.get::<_, String>(0), "reason": mark.reason, "nodes": mark.nodes}),
    )
}

#[cfg(test)]
mod tests {
    use super::{page_limit, parse_mark};
    use serde_json::json;

    #[test]
    fn a_history_page_holds_1_to_25_entries() {
        assert_eq!(page_limit(&json!({})).unwrap(), 25);
        assert_eq!(page_limit(&json!({"limit": 1})).unwrap(), 1);
        assert_eq!(page_limit(&json!({"limit": 25})).unwrap(), 25);
        for bad in [json!(0), json!(26), json!("5")] {
            assert!(
                page_limit(&json!({"limit": bad}))
                    .unwrap_err()
                    .to_string()
                    .contains("limit")
            );
        }
    }

    #[test]
    fn a_mark_refuses_actor_at_16_nodes_and_repeats() {
        let nodes: Vec<String> = (0..16).map(|i| format!("task:t{i}")).collect();
        for (input, error) in [
            (
                json!({"position": 2, "reason": "Done", "actor": "x"}),
                "actor comes from the server",
            ),
            (
                json!({"position": 2, "reason": "Done", "at": "2020-01-01T00:00:00Z"}),
                "at comes from the server",
            ),
            (
                json!({"position": 2, "reason": "Done", "nodes": nodes}),
                "at most 15 nodes",
            ),
            (
                json!({"position": 2, "reason": "Done", "nodes": ["task:a", "task:a"]}),
                "each node once",
            ),
            (json!({"position": 2, "reason": " "}), "needs a reason"),
            (json!({"position": 0, "reason": "Done"}), "journal position"),
        ] {
            let message = parse_mark(&input).unwrap_err().to_string();
            assert!(message.contains(error), "{input}: {message}");
        }
        assert!(
            parse_mark(
                &json!({"workspace": "w", "position": 2, "reason": "Cut", "nodes": ["task:a"]})
            )
            .is_ok()
        );
    }
}
