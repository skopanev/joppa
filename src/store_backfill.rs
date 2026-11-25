//! The board projection backfill at serve start, and the status projection catch-up after it.
use super::*;

/// The board rows of a workspace are stale: only a journal replay rebuilds them. The server
/// writes by these rows, so this backfill runs before the server takes requests.
const BOARD_STALE: &str = "b.workspace IS NULL OR b.first_view='{}'::jsonb OR NOT EXISTS(SELECT 1 FROM joppa.state_entries s WHERE s.workspace=w.id AND s.section='meta') OR NOT (b.first_view ? 'object_types') OR COALESCE(b.first_view->>'projection_version','')<>$1 OR b.position<>(SELECT COALESCE(MAX(e.position),0) FROM joppa.events e WHERE e.workspace=w.id)";

/// The status projection of a workspace is from an older code version or position, or has a
/// row without a stored value.
const STATUS_STALE: &str = "COALESCE((b.first_view->'status_projection'->>'version')::int,0)<>$1 OR COALESCE((b.first_view->'status_projection'->>'position')::bigint,-1)<>b.position OR EXISTS(SELECT 1 FROM joppa.objects o WHERE o.workspace=w.id AND (o.test_status IS NULL OR o.rollup IS NULL))";

pub async fn backfill_board(pool: &Pool) -> Result<usize> {
    let mut conn = pool.get().await?;
    let query = format!(
        "SELECT w.id FROM joppa.workspaces w LEFT JOIN joppa.board_cache b ON b.workspace=w.id WHERE {BOARD_STALE} ORDER BY w.id"
    );
    let rows = conn.query(&query, &[&BOARD_PROJECTION_VERSION]).await?;
    for row in &rows {
        let id: String = row.get(0);
        let tx = conn.transaction().await?;
        lock(&*tx, &id).await?;
        replay(&*tx, &id).await?;
        tx.commit().await?;
        super::stream::trim_heap();
    }
    Ok(rows.len())
}

/// Rebuild every projection of a workspace from its journal.
async fn replay(tx: &impl GenericClient, id: &str) -> Result<()> {
    // A rebuild writes the journal as it is: older revisions may hold longer fields.
    tx.batch_execute("SET LOCAL joppa.replaying = '1'").await?;
    let (state, events) = load(tx, id).await?;
    board::project(tx, id, &state, &events).await?;
    write::migrate_legacy_receipts_workspace(tx, id).await?;
    let position = events.last().map_or(0, |e| e.position);
    search::project(tx, id, &state, position, None).await
}

/// Rebuild the status projection from the current rows, one workspace in each transaction,
/// while the server reads the stored values. Without `all`, only the stale workspaces.
pub async fn catch_up_statuses(pool: &Pool, all: bool) -> Result<usize> {
    let mut conn = pool.get().await?;
    let query = format!(
        "SELECT w.id FROM joppa.workspaces w JOIN joppa.board_cache b ON b.workspace=w.id WHERE $2 OR {STATUS_STALE} ORDER BY w.id"
    );
    let rows = conn
        .query(&query, &[&objects::STATUS_PROJECTION_VERSION, &all])
        .await?;
    let mut done = 0;
    for row in rows {
        let id: String = row.get(0);
        let tx = conn.transaction().await?;
        lock(&*tx, &id).await?;
        // Current rows that do not read, such as a damaged entry, rebuild from the journal.
        match state_rows::current(&*tx, &id).await {
            Ok((state, position)) => {
                objects::rebuild_status_projection(&*tx, &id, &state, position).await?
            }
            Err(_) => replay(&*tx, &id).await?,
        }
        tx.commit().await?;
        super::stream::trim_heap();
        done += 1;
    }
    Ok(done)
}

/// The catch-up of the stale workspaces at serve start, then each day. A full rebuild of
/// every workspace reads every test result of a large workspace at once and holds its write
/// lock, so the loop runs none. The writes keep the projection equal to a full rebuild.
pub async fn status_projection_loop(pool: Pool) {
    loop {
        if let Err(e) = catch_up_statuses(&pool, false).await {
            eprintln!("status projection catch-up failed: {e:#}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
    }
}

/// Fill parent_hash_current of each object (parent_hash.rs) in the workspaces with an older
/// version, one workspace in each transaction, in parts of PART rows.
pub async fn catch_up_parent_hashes(pool: &Pool) -> Result<usize> {
    let mut conn = pool.get().await?;
    let rows = conn.query("SELECT w.id FROM joppa.workspaces w JOIN joppa.board_cache b ON b.workspace=w.id WHERE COALESCE(b.first_view->>'parent_hash_version','')<>$1 ORDER BY w.id", &[&PARENT_HASH_VERSION]).await?;
    for row in &rows {
        let id: String = row.get(0);
        let tx = conn.transaction().await?;
        lock(&*tx, &id).await?;
        let (state, _) = state_rows::current(&*tx, &id).await?;
        let keys: Vec<&String> = state.objects.keys().collect();
        for part in keys.chunks(super::stream::PART) {
            let values: Vec<Value> = part
                .iter()
                .map(|a| json!({"address": a, "hash": crate::objects::parent_hash_current(&state, a)}))
                .collect();
            tx.execute("UPDATE joppa.objects o SET parent_hash_current=x.hash FROM jsonb_to_recordset($2::text::jsonb) AS x(address text,hash text) WHERE o.workspace=$1 AND o.address=x.address AND o.parent_hash_current IS DISTINCT FROM x.hash", &[&id, &json!(values).to_string()]).await?;
        }
        tx.execute("UPDATE joppa.board_cache SET first_view=first_view||jsonb_build_object('parent_hash_version',$2::text) WHERE workspace=$1", &[&id, &PARENT_HASH_VERSION]).await?;
        tx.commit().await?;
        drop(keys);
        drop(state);
        super::stream::trim_heap();
    }
    Ok(rows.len())
}

/// The write lock of one workspace.
async fn lock(client: &impl GenericClient, id: &str) -> Result<()> {
    client
        .execute(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
            &[&id],
        )
        .await?;
    Ok(())
}
