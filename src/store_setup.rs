//! Database setup at serve start: the pool, migrations, notes, readiness metadata and the board backfill.
use super::*;

pub fn pool(url: &str) -> Result<Pool> {
    let mut cfg = deadpool_postgres::Config::new();
    cfg.url = Some(url.to_string());
    // The writers of a workspace wait before they take a connection (store_writer.rs).
    cfg.pool = Some(deadpool_postgres::PoolConfig::new(32));
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
    // Migrations rebuild projections from the journal as it is (see migration 037).
    tx.batch_execute("SET LOCAL joppa.replaying = '1'").await?;
    // The DDL files run first, in their number order, with the pass of 061. Then the data
    // passes 062, 063 and 064 run.
    for sql in [
        include_str!("../migrations/001_journal.sql"),
        include_str!("../migrations/002_item_ids.sql"),
        include_str!("../migrations/003_reference_prefix.sql"),
        include_str!("../migrations/004_cli_credentials.sql"),
        include_str!("../migrations/005_mcp_oauth.sql"),
        include_str!("../migrations/006_workspace_aliases.sql"),
        include_str!("../migrations/007_command_receipts.sql"),
        include_str!("../migrations/008_search.sql"),
        include_str!("../migrations/010_consumers.sql"),
        include_str!("../migrations/011_board_cache.sql"),
        include_str!("../migrations/012_objects.sql"),
        include_str!("../migrations/013_object_aliases.sql"),
        include_str!("../migrations/014_identity_names.sql"),
        include_str!("../migrations/015_current_state.sql"),
        include_str!("../migrations/016_current_item_references.sql"),
        include_str!("../migrations/017_test_reports.sql"),
        include_str!("../migrations/019_inbox_order.sql"),
        include_str!("../migrations/022_email_grants.sql"),
        include_str!("../migrations/023_test_report_receipts.sql"),
        include_str!("../migrations/024_domain_grants.sql"),
        include_str!("../migrations/025_session_hd.sql"),
        include_str!("../migrations/026_replay_epochs.sql"),
        include_str!("../migrations/027_status_projection.sql"),
        include_str!("../migrations/028_recorded_results.sql"),
        include_str!("../migrations/029_history_marks.sql"),
        include_str!("../migrations/030_human_members_epoch.sql"),
        include_str!("../migrations/031_owner_writes_epoch.sql"),
        include_str!("../migrations/032_single_criterion_save_epoch.sql"),
        include_str!("../migrations/033_purge_workspace.sql"),
        include_str!("../migrations/034_criterion_save_reason_epoch.sql"),
        include_str!("../migrations/035_test_result_fields.sql"),
        include_str!("../migrations/036_hybrid_search.sql"),
        include_str!("../migrations/037_field_length_limit.sql"),
        include_str!("../migrations/038_file_storage.sql"),
        include_str!("../migrations/039_objects_data_gin.sql"),
        include_str!("../migrations/040_short_item_ids.sql"),
        include_str!("../migrations/041_item_alias_ends.sql"),
        include_str!("../migrations/042_short_item_id_codes.sql"),
        include_str!("../migrations/043_no_purpose_no_ac_owners.sql"),
        include_str!("../migrations/044_archive_reset.sql"),
        include_str!("../migrations/045_node_filter_indexes.sql"),
        include_str!("../migrations/046_generic_nodes.sql"),
        include_str!("../migrations/047_own_mark_reset.sql"),
        include_str!("../migrations/048_update_workspace.sql"),
        include_str!("../migrations/049_actor_alias.sql"),
        include_str!("../migrations/050_no_agent_tokens.sql"),
        include_str!("../migrations/051_any_change_reset.sql"),
        include_str!("../migrations/052_filter_dimensions.sql"),
        include_str!("../migrations/053_computed_fields.sql"),
        include_str!("../migrations/054_no_readiness_marks.sql"),
        include_str!("../migrations/055_computed_index.sql"),
        include_str!("../migrations/056_forced_keeps_contract.sql"),
        include_str!("../migrations/057_no_launch_summary.sql"),
        include_str!("../migrations/058_test_node_records.sql"),
        include_str!("../migrations/059_search_and_reference_indexes.sql"),
        include_str!("../migrations/060_source_change_reset.sql"),
        HIDDEN_BY,
        include_str!("../migrations/062_relations.sql"),
        include_str!("../migrations/065_no_link_blocking.sql"),
    ] {
        tx.batch_execute(sql).await?;
    }
    relations_data(&*tx).await?;
    tx.batch_execute(BLUE_TONE).await?;
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
/// Migration 061 moves the older hidden flag without a replay. It runs at serve start too: a
/// server that starts without `joppa migrate` reads no older flag.
const HIDDEN_BY: &str = include_str!("../migrations/061_hidden_by.sql");
/// The data pass of migration 062 moves the stored links to relations without a replay. At
/// serve start it runs only when a workspace has no relations_reset epoch or a state row has no
/// relations, and it needs no DDL: the application role runs it.
const RELATIONS: &str = include_str!("../migrations/062_relations_data.sql");
const RELATIONS_PENDING: &str = "SELECT EXISTS(SELECT 1 FROM joppa.workspaces w WHERE NOT EXISTS(SELECT 1 FROM joppa.replay_epochs e WHERE e.workspace=w.id AND e.rule='relations_reset')) OR EXISTS(SELECT 1 FROM joppa.state_entries WHERE section='objects' AND NOT value->'revisions'->0 ? 'relations')";
/// Migration 063 moves the older tone of the current rows to blue without a replay. It
/// needs no DDL and runs at serve start too. A second run writes nothing.
const BLUE_TONE: &str = include_str!("../migrations/063_blue_tone.sql");
async fn relations_data(client: &impl GenericClient) -> Result<()> {
    if client.query_one(RELATIONS_PENDING, &[]).await?.get(0) {
        client.batch_execute(RELATIONS).await?;
    }
    Ok(())
}
pub async fn migrate_notes(pool: &Pool) -> Result<()> {
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute("SELECT pg_advisory_xact_lock(91827101)", &[])
        .await?;
    // Migrations rebuild projections from the journal as it is (see migration 037).
    tx.batch_execute("SET LOCAL joppa.replaying = '1'").await?;
    tx.batch_execute(HIDDEN_BY).await?;
    relations_data(&*tx).await?;
    tx.batch_execute(BLUE_TONE).await?;
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
        .query("SELECT w.id FROM joppa.workspaces w JOIN joppa.board_cache b ON b.workspace=w.id WHERE NOT COALESCE((b.state->>'notes_migrated')::boolean,false) OR NOT COALESCE((b.state->>'tasks_migrated')::boolean,false) OR NOT COALESCE((b.state->>'schema_migrated')::boolean,false) OR NOT COALESCE((b.state->>'documents_migrated')::boolean,false) OR NOT COALESCE((b.state->>'claims_migrated')::boolean,false) OR NOT COALESCE((b.state->>'persistent_claims')::boolean,false) OR NOT COALESCE((b.state->>'consumers_migrated')::boolean,false) OR NOT COALESCE((b.state->>'consumer_drops_noted')::boolean,false) OR NOT COALESCE((b.state->>'test_rules_migrated')::boolean,false) OR NOT COALESCE((b.state->>'note_parents_migrated')::boolean,false) OR NOT COALESCE((b.state->>'body_migrated')::boolean,false) OR NOT COALESCE((b.state->>'purpose_migrated')::boolean,false) OR NOT COALESCE((b.state->>'test_nodes_migrated')::boolean,false) OR NOT COALESCE((b.state->>'generic_nodes')::boolean,false) OR NOT COALESCE((b.state->>'decision_required_migrated')::boolean,false) OR ((b.state->>'reference_prefix' IS NULL OR COALESCE((b.state->>'prefix_from_title')::boolean,false)) AND COALESCE(b.state->>'reference_prefix',w.id) NOT LIKE trim(both '-' from left(lower(regexp_replace(COALESCE(b.state->>'title',''),'[^A-Za-z0-9]+','-','g')),40))||'%') ORDER BY w.id", &[])
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
                    || o.current().depends_on().any(|edge| selected.contains(crate::objects::resolve_address(&state,&edge.to)))
                    || o.current().data["depends_on"].as_array().is_some_and(|deps| deps.iter().filter_map(Value::as_str).any(|key| selected.contains(&crate::objects::criterion_address(key)))))
            }).map(|(key,_)|key.clone()).collect();
            if dependents.is_empty() { break; }
            selected.extend(dependents);
        }
        objects::project(client, &workspace, &state, &[], Some(&selected)).await?;
        client.execute("UPDATE joppa.board_cache SET first_view=jsonb_set(first_view,'{readiness_metadata_version}',to_jsonb($2::text)) WHERE workspace=$1", &[&workspace,&READINESS_METADATA_VERSION]).await?;
    }
    Ok(())
}

#[path = "store_backfill.rs"]
mod backfill;
pub use backfill::{backfill_board, catch_up_statuses, status_projection_loop};
