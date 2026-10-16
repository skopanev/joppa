//! The workspace timeline read and the journal event of a test report upload.
//! A report upload appends a record_report event, so the journal order holds every
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
    let more = rows.len() > scope.limit;
    let rows = &rows[..rows.len().min(scope.limit)];
    let commands: Vec<Value> = rows
        .iter()
        .map(|row| serde_json::from_str(row.get(3)))
        .collect::<std::result::Result<_, _>>()?;
    let positions: Vec<i64> = rows.iter().map(|row| row.get(0)).collect();
    let addresses: Vec<Option<String>> = commands.iter().map(crate::timeline::address).collect();
    // The object, its revision at the event and the revision before that one.
    let objects = conn.query(
        "SELECT x.i, o.address, o.kind, o.title, r.revision, r.document::text, p.document::text
         FROM unnest($2::text[], $3::bigint[]) WITH ORDINALITY AS x(address, position, i)
         LEFT JOIN joppa.object_aliases a ON a.workspace=$1 AND a.address=x.address
         JOIN joppa.objects o ON o.workspace=$1 AND o.address=COALESCE(a.target, x.address)
         LEFT JOIN LATERAL (SELECT revision, document FROM joppa.object_revisions
             WHERE workspace=$1 AND address=o.address AND (document->>'position')::bigint<=x.position
             ORDER BY revision DESC LIMIT 1) r ON true
         LEFT JOIN joppa.object_revisions p ON p.workspace=$1 AND p.address=o.address AND p.revision=r.revision-1",
        &[&workspace, &addresses, &positions],
    ).await?;
    let mut found = std::collections::BTreeMap::new();
    for row in &objects {
        let document = |i| {
            row.get::<_, Option<&str>>(i)
                .map(serde_json::from_str::<Value>)
                .transpose()
        };
        found.insert(
            row.get::<_, i64>(0) as usize - 1,
            (
                row.get::<_, String>(1),
                row.get::<_, String>(2),
                row.get::<_, String>(3),
                row.get::<_, Option<i32>>(4),
                document(5)?,
                document(6)?,
            ),
        );
    }
    // The journal keeps redacted fragments; the timeline masks them in every text it returns.
    let fragments: Vec<String> = conn
        .query(
            "SELECT command->>'fragment' FROM joppa.events WHERE workspace=$1 AND command->>'op'='redact_object' AND command->>'fragment'<>''",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    let mask = |text: &str| {
        fragments.iter().fold(text.to_owned(), |text, fragment| {
            text.replace(fragment, crate::redact::MASK)
        })
    };
    let actors: Vec<String> = rows
        .iter()
        .map(|row| row.get::<_, String>(2))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let types = access::actor_types(&*conn, &workspace, &actors).await?;
    let mut items = vec![];
    for (i, (row, command)) in rows.iter().zip(&commands).enumerate() {
        let position: i64 = row.get(0);
        let actor: String = row.get(2);
        let mut change = crate::timeline::change(command);
        let mut reason = crate::timeline::reason(command);
        let mut item = json!({
            "position": position,
            "at": row.get::<_, String>(1),
            "actor": actor,
            "actor_type": types[&actor],
            "op": command["op"],
            "address": addresses[i],
            "kind": null,
            "title": null,
            "revision": null,
        });
        if command["op"] == "record_report" {
            let commit = command["commit"].as_str().unwrap_or_default();
            item["title"] = json!(format!("Test report {}", &commit[..commit.len().min(12)]));
            item["report"] = command["report"].clone();
        }
        if let Some((address, kind, title, revision, saved, previous)) = found.get(&i) {
            item["address"] = json!(address);
            item["kind"] = json!(kind);
            item["revision"] = json!(revision);
            item["title"] = json!(
                saved
                    .as_ref()
                    .and_then(|d| d["data"]["title"].as_str())
                    .unwrap_or(title)
            );
            // A revision that this event made gives the reason in its stored, masked form.
            if let Some(saved) = saved.as_ref().filter(|d| d["position"] == position) {
                if let Some(text) = saved["reason"].as_str() {
                    reason = text.to_owned();
                }
                if command["op"] == "save_object"
                    && previous
                        .as_ref()
                        .is_some_and(|p| crate::timeline::moved(saved, p))
                {
                    change = "move";
                }
            }
        }
        item["change"] = json!(change);
        item["reason"] = json!(mask(&reason));
        if let Some(title) = item["title"].as_str() {
            item["title"] = json!(mask(title));
        }
        items.push(item);
    }
    let next_cursor = (more && !rows.is_empty())
        .then(|| format!("p{}", rows.last().map(|r| r.get::<_, i64>(0)).unwrap_or(0)));
    Ok(json!({
        "workspace": public,
        "position": head,
        "after": scope.after,
        "order": if newest { "newest" } else { "oldest" },
        "items": items,
        "next_cursor": next_cursor,
    }))
}

/// Append the record_report event of one accepted upload. The caller holds the
/// transaction; a failed append rolls back the upload.
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
