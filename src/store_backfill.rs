//! The board projection backfill at serve start.
use super::*;

pub async fn backfill_board(pool: &Pool) -> Result<usize> {
    let mut conn = pool.get().await?;
    // `board` is true when the board rows are stale and only a journal replay rebuilds them.
    // A stale status projection alone rebuilds from the current rows.
    let rows = conn.query("SELECT id,board FROM (SELECT w.id,(b.workspace IS NULL OR b.first_view='{}'::jsonb OR NOT EXISTS(SELECT 1 FROM joppa.state_entries s WHERE s.workspace=w.id AND s.section='meta') OR NOT (b.first_view ? 'object_types') OR COALESCE(b.first_view->>'projection_version','')<>$1 OR b.position<>(SELECT COALESCE(MAX(e.position),0) FROM joppa.events e WHERE e.workspace=w.id)) AS board,(COALESCE((b.first_view->'status_projection'->>'version')::int,0)<>$2 OR COALESCE((b.first_view->'status_projection'->>'position')::bigint,-1)<>b.position OR EXISTS(SELECT 1 FROM joppa.objects o WHERE o.workspace=w.id AND (o.test_status IS NULL OR o.rollup IS NULL))) AS status FROM joppa.workspaces w LEFT JOIN joppa.board_cache b ON b.workspace=w.id) x WHERE board OR status ORDER BY id", &[&BOARD_PROJECTION_VERSION, &objects::STATUS_PROJECTION_VERSION]).await?;
    let mut done = 0;
    for row in rows {
        let (id, board): (String, bool) = (row.get(0), row.get(1));
        let tx = conn.transaction().await?;
        tx.execute(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
            &[&id],
        )
        .await?;
        // A rebuild writes the journal as it is: older revisions may hold longer fields.
        tx.batch_execute("SET LOCAL joppa.replaying = '1'").await?;
        // Current rows that do not read, such as a damaged entry, rebuild from the journal.
        if !board && let Ok((state, position)) = state_rows::current(&*tx, &id).await {
            objects::rebuild_status_projection(&*tx, &id, &state, position).await?;
            write::migrate_legacy_receipts_workspace(&*tx, &id).await?;
            tx.commit().await?;
            done += 1;
            continue;
        }
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
