//! Database setup at serve start: the pool, migrations, notes, readiness metadata and the board backfill.
use super::*;

pub fn pool(url: &str) -> Result<Pool> {
    let mut cfg = deadpool_postgres::Config::new();
    cfg.url = Some(url.to_string());
    Ok(cfg.create_pool(
        Some(deadpool_postgres::Runtime::Tokio1),
        tokio_postgres::NoTls,
    )?)
}
/// Migrations are repeatable, and Postgres aborts one side of a deadlock with a running
/// transaction (SQLSTATE 40P01). A deadlocked migration simply runs again.
pub async fn migrate(pool: &Pool) -> Result<()> {
    for attempt in 1.. {
        match migrate_once(pool).await {
            Err(e) if attempt < 3 && is_deadlock(&e) => {
                // Visible on purpose: a retried migration must show up in the logs.
                eprintln!("joppa: migration attempt {attempt} hit a deadlock, retrying: {e:#}");
                tokio::time::sleep(std::time::Duration::from_millis(100 * attempt)).await;
            }
            done => return done,
        }
    }
    unreachable!()
}
fn is_deadlock(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| {
        cause
            .downcast_ref::<tokio_postgres::Error>()
            .and_then(tokio_postgres::Error::code)
            .is_some_and(|code| *code == tokio_postgres::error::SqlState::T_R_DEADLOCK_DETECTED)
    })
}
async fn migrate_once(pool: &Pool) -> Result<()> {
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute("SELECT pg_advisory_xact_lock(91827101)", &[])
        .await?;
    tx.batch_execute(include_str!("../migrations/001_journal.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/002_item_ids.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/003_reference_prefix.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/004_cli_credentials.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/005_mcp_oauth.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/006_workspace_aliases.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/007_command_receipts.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/008_search.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/009_annotations.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/010_consumers.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/011_board_cache.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/012_objects.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/013_object_aliases.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/014_identity_names.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/015_current_state.sql"))
        .await?;
    tx.batch_execute(include_str!(
        "../migrations/016_current_item_references.sql"
    ))
    .await?;
    tx.batch_execute(include_str!("../migrations/017_test_reports.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/018_agent_tokens.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/019_inbox_order.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/020_task_claims.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/021_agent_token_access.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/022_email_grants.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/023_test_report_receipts.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/024_domain_grants.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/025_session_hd.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/026_replay_epochs.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/027_status_projection.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/028_recorded_results.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/029_history_marks.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/030_human_members_epoch.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/031_owner_writes_epoch.sql"))
        .await?;
    tx.batch_execute(include_str!(
        "../migrations/032_single_criterion_save_epoch.sql"
    ))
    .await?;
    tx.batch_execute(include_str!("../migrations/033_purge_workspace.sql"))
        .await?;
    tx.batch_execute(include_str!(
        "../migrations/034_criterion_save_reason_epoch.sql"
    ))
    .await?;
    tx.batch_execute(include_str!("../migrations/035_test_result_fields.sql"))
        .await?;
    for row in tx.query("SELECT w.id FROM joppa.workspaces w LEFT JOIN joppa.board_cache b ON b.workspace=w.id WHERE b.workspace IS NULL OR b.first_view='{}'::jsonb OR NOT EXISTS(SELECT 1 FROM joppa.state_entries s WHERE s.workspace=w.id AND s.section='meta') OR NOT (b.first_view ? 'object_types') OR COALESCE(b.first_view->>'projection_version','')<>$1", &[&BOARD_PROJECTION_VERSION]).await? {
        let id: String = row.get(0);
        tx.execute("SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))", &[&id]).await?;
        let (state, events) = load(&*tx, &id).await?;
        board::project(&*tx, &id, &state, &events).await?;
        search::project(&*tx, &id, &state, events.last().map_or(0, |e| e.position), None).await?;
    }
    migrate_notes_in(&*tx).await?;
    refresh_readiness_in(&*tx).await?;
    write::migrate_legacy_receipts(&*tx).await?;
    super::references::remove_dangling(&*tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn migrate_notes(pool: &Pool) -> Result<()> {
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute("SELECT pg_advisory_xact_lock(91827101)", &[])
        .await?;
    migrate_notes_in(&*tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn check_journal(pool: &Pool) -> Result<()> {
    let conn = pool.get().await?;
    for row in conn
        .query("SELECT id FROM joppa.workspaces ORDER BY id", &[])
        .await?
    {
        let _: (Workspace, Vec<Event>) = load(&**conn, row.get(0)).await?;
    }
    Ok(())
}
async fn migrate_notes_in(client: &impl GenericClient) -> Result<()> {
    for row in client
        .query("SELECT w.id FROM joppa.workspaces w JOIN joppa.board_cache b ON b.workspace=w.id WHERE NOT COALESCE((b.state->>'notes_migrated')::boolean,false) OR NOT COALESCE((b.state->>'tasks_migrated')::boolean,false) OR NOT COALESCE((b.state->>'schema_migrated')::boolean,false) OR NOT COALESCE((b.state->>'documents_migrated')::boolean,false) OR NOT COALESCE((b.state->>'claims_migrated')::boolean,false) OR NOT COALESCE((b.state->>'persistent_claims')::boolean,false) OR NOT COALESCE((b.state->>'consumers_migrated')::boolean,false) OR NOT COALESCE((b.state->>'consumer_drops_noted')::boolean,false) OR NOT COALESCE((b.state->>'test_rules_migrated')::boolean,false) ORDER BY w.id", &[])
        .await?
    {
        let workspace: String = row.get(0);
        client
            .execute(
                "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
                &[&workspace],
            )
            .await?;
        let (mut state, old_events) = state_rows::full(client, &workspace).await?;
        let position = old_events.last().map_or(0, |e|e.position);
        let previous = state.clone();
        let mut events = Vec::new();
        if ensure_notes(client, &workspace, &mut state, &mut events, position).await? {
            search::project(
                client,
                &workspace,
                &state,
                position + events.len() as u64,
                None,
            )
            .await?;
            board::project_current(
                client,
                &workspace,
                &state,
                &events,
                position + events.len() as u64,
                Some(&previous),
            )
            .await?;
        }
    }
    Ok(())
}

/// Refresh current assessment and verification metadata without reading journal history.
pub async fn refresh_readiness_metadata(pool: &Pool) -> Result<()> {
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute("SELECT pg_advisory_xact_lock(91827101)", &[])
        .await?;
    refresh_readiness_in(&*tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn refresh_readiness_in(client: &impl GenericClient) -> Result<()> {
    for row in client.query("SELECT workspace FROM joppa.board_cache WHERE COALESCE(first_view->>'readiness_metadata_version','')<>$1 ORDER BY workspace", &[&READINESS_METADATA_VERSION]).await? {
        let workspace: String = row.get(0);
        client.execute("SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))", &[&workspace]).await?;
        let (state, _) = state_rows::current(client, &workspace).await?;
        let schema = crate::objects::schema(&state);
        let mut selected: std::collections::BTreeSet<_> = state.objects.iter().filter(|(_, o)| {
            schema.types.get(&o.kind).is_some_and(|t| matches!(t.role.as_str(), "ac" | "task"))
        }).map(|(key, _)| key.clone()).collect();
        loop {
            let dependents: Vec<_> = state.objects.iter().filter(|(key, o)| {
                !selected.contains(*key) && (o.current().parent.as_deref().is_some_and(|parent| selected.contains(crate::objects::resolve_address(&state,parent)))
                    || o.current().links.iter().any(|link| link.relation == "depends_on" && selected.contains(crate::objects::resolve_address(&state,&link.target)))
                    || o.current().data["depends_on"].as_array().is_some_and(|deps| deps.iter().filter_map(Value::as_str).any(|key| selected.contains(&crate::objects::criterion_address(key)))))
            }).map(|(key,_)|key.clone()).collect();
            if dependents.is_empty() { break; }
            selected.extend(dependents);
        }
        objects::project(client, &workspace, &state, &[], Some(&selected)).await?;
        queue_rows::project(client, &workspace, &state, Some(&selected)).await?;
        client.execute("UPDATE joppa.board_cache SET first_view=jsonb_set(first_view,'{readiness_metadata_version}',to_jsonb($2::text)) WHERE workspace=$1", &[&workspace,&READINESS_METADATA_VERSION]).await?;
    }
    Ok(())
}

pub async fn backfill_board(pool: &Pool) -> Result<usize> {
    let mut conn = pool.get().await?;
    let rows = conn.query("SELECT w.id FROM joppa.workspaces w LEFT JOIN joppa.board_cache b ON b.workspace=w.id WHERE b.workspace IS NULL OR b.first_view='{}'::jsonb OR NOT EXISTS(SELECT 1 FROM joppa.state_entries s WHERE s.workspace=w.id AND s.section='meta') OR NOT (b.first_view ? 'object_types') OR COALESCE(b.first_view->>'projection_version','')<>$1 OR b.position<>(SELECT COALESCE(MAX(e.position),0) FROM joppa.events e WHERE e.workspace=w.id) OR EXISTS(SELECT 1 FROM joppa.task_queue q WHERE q.workspace=w.id AND q.created_at=0) OR COALESCE((b.first_view->'status_projection'->>'version')::int,0)<>$2 OR COALESCE((b.first_view->'status_projection'->>'position')::bigint,-1)<>b.position OR EXISTS(SELECT 1 FROM joppa.objects o WHERE o.workspace=w.id AND (o.test_status IS NULL OR o.rollup IS NULL)) ORDER BY w.id", &[&BOARD_PROJECTION_VERSION, &objects::STATUS_PROJECTION_VERSION]).await?;
    let mut done = 0;
    for row in rows {
        let id: String = row.get(0);
        let tx = conn.transaction().await?;
        tx.execute(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
            &[&id],
        )
        .await?;
        let (state, events) = load(&*tx, &id).await?;
        board::project(&*tx, &id, &state, &events).await?;
        write::migrate_legacy_receipts_workspace(&*tx, &id).await?;
        search::project(
            &*tx,
            &id,
            &state,
            events.last().map_or(0, |e| e.position),
            None,
        )
        .await?;
        tx.commit().await?;
        done += 1;
    }
    Ok(done)
}
