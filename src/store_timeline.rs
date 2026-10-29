//! The workspace timeline read and the journal event of a record of test results.
//! A record appends a record_report event, so the journal order holds every
//! recorded change and a timeline position is always a journal position.
use super::*;

/// The largest timeline page.
pub const TIMELINE_LIMIT: usize = 100;

/// One timeline read. `after` is the last position that the reader knows.
/// `order` is "oldest" (journal order, the default) or "newest" (reverse journal order).
pub struct TimelineScope<'a> {
    pub after: u64,
    pub limit: usize,
    pub cursor: Option<&'a str>,
    pub order: Option<&'a str>,
}

/// A cursor holds the last position of the page that it continues.
fn cursor_position(cursor: &str) -> Result<u64> {
    cursor
        .strip_prefix("p")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| commands::ClientField("invalid timeline cursor").into())
}

/// Read the entries after a position: one entry for each journal event.
/// Members and agents of the workspace read it; another actor gets "Workspace not found".
pub async fn read_timeline(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    scope: &TimelineScope<'_>,
) -> Result<Value> {
    let newest = match scope.order.unwrap_or("oldest") {
        "oldest" => false,
        "newest" => true,
        _ => return Err(commands::ClientField("order must be oldest or newest").into()),
    };
    if !(1..=TIMELINE_LIMIT).contains(&scope.limit) {
        return Err(commands::ClientField("limit must be between 1 and 100").into());
    }
    let mut conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    let conn = conn
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let row = conn
        .query_one(
            "SELECT COALESCE(state->>'workspace_id',workspace),position FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    let public: String = row.get(0);
    let head = row.get::<_, i64>(1);
    let after = i64::try_from(scope.after)?;
    // Oldest first continues after the cursor; newest first continues before it.
    let (low, high) = match (scope.cursor.map(cursor_position).transpose()?, newest) {
        (Some(p), false) => (after.max(i64::try_from(p)?), head),
        (Some(p), true) => (after, i64::try_from(p)? - 1),
        (None, _) => (after, head),
    };
    let rows = conn
        .query(
            if newest {
                "SELECT position,recorded_at::text,actor,command::text FROM joppa.events WHERE workspace=$1 AND position>$2 AND position<=$3 ORDER BY position DESC LIMIT $4"
            } else {
                "SELECT position,recorded_at::text,actor,command::text FROM joppa.events WHERE workspace=$1 AND position>$2 AND position<=$3 ORDER BY position LIMIT $4"
            },
            &[&workspace, &low, &high, &(scope.limit as i64 + 1)],
        )
        .await?;
    // `total` counts every entry after `after`, also the entries before the cursor.
    let total: i64 = conn
        .query_one(
            "SELECT count(*) FROM joppa.events WHERE workspace=$1 AND position>$2 AND position<=$3",
            &[&workspace, &after, &head],
        )
        .await?
        .get(0);
    let more = rows.len() > scope.limit;
    let rows = &rows[..rows.len().min(scope.limit)];
    let items = super::timeline_items::items(&*conn, &workspace, rows).await?;
    let next_cursor = (more && !rows.is_empty())
        .then(|| format!("p{}", rows.last().map(|r| r.get::<_, i64>(0)).unwrap_or(0)));
    Ok(json!({
        "workspace": public,
        "position": head,
        "after": scope.after,
        "order": if newest { "newest" } else { "oldest" },
        "total": total,
        "items": items,
        "next_cursor": next_cursor,
    }))
}

/// Append the record_report event of one accepted record. The caller holds the
/// transaction; a failed append rolls back the record.
pub(super) async fn record_report(
    tx: &impl GenericClient,
    workspace: &str,
    actor: &str,
    command: Command,
) -> Result<u64> {
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&workspace],
    )
    .await?;
    let (mut state, base_position) = state_rows::current(tx, workspace).await?;
    let previous = state.clone();
    let position = base_position + 1;
    let recorded_at: String = tx
        .query_one(
            "INSERT INTO joppa.events(workspace,position,request_id,actor,command) VALUES($1,$2,$3,$4,$5::text::jsonb) RETURNING recorded_at::text",
            &[
                &workspace,
                &(position as i64),
                &format!("report-{}", uuid::Uuid::new_v4()),
                &actor,
                &serde_json::to_string(&command)?,
            ],
        )
        .await?
        .get(0);
    commands::apply(&mut state, actor, &command, position, &recorded_at)?;
    let events = [Event {
        position,
        actor: actor.into(),
        actor_type: crate::inbox::actor_type(&state, actor).into(),
        recorded_at,
        command,
    }];
    let selected = state_rows::affected(&state, Some(&previous))?;
    search::project(tx, workspace, &state, position, selected.as_ref()).await?;
    board::project_current(tx, workspace, &state, &events, position, Some(&previous)).await?;
    Ok(position)
}
