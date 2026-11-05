//! requirement:precomputed-status-reads: the backfill at start replays the journal only for
//! stale board rows, and the catch-up after the start rebuilds a stale status projection from
//! the current rows.
use anyhow::{Result, ensure};
use joppa::{probe, store};
use serde_json::{Value, json};

/// The stored status of each object of the workspace.
async fn statuses(
    pool: &deadpool_postgres::Pool,
    ws: &str,
) -> Result<Vec<(String, String, String)>> {
    let sql = "SELECT address,test_status::text,rollup::text FROM joppa.objects WHERE workspace=$1 ORDER BY address";
    let rows = pool.get().await?.query(sql, &[&ws]).await?;
    Ok(rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect())
}

/// The journal replays and status rebuilds of one call.
async fn counted(call: impl std::future::Future<Output = Result<usize>>) -> Result<(u64, u64)> {
    let (replays, rebuilds) = (probe::read(&probe::REPLAY), probe::read(&probe::REBUILD));
    call.await?;
    Ok((
        probe::read(&probe::REPLAY) - replays,
        probe::read(&probe::REBUILD) - rebuilds,
    ))
}

// ac:be6tvcf6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_stale_status_projection_catches_up_after_the_start_without_a_replay() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    store::backfill_board(&pool).await?;
    let ws = format!("backfill-{}", uuid::Uuid::new_v4().simple());
    let op = serde_json::from_value(json!({"op":"workspace","title":"Backfill"}))?;
    store::execute(&pool, &ws, "owner", "setup-0", &op).await?;
    let write = async |command: Value| {
        let id = uuid::Uuid::new_v4().to_string();
        store::execute_remote_input(&pool, &ws, "owner", &id, &command).await
    };
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    write(json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
        "ac":{"title":"Criteria","role":"ac","status":"children","parents":["requirement"]},
        "test":{"title":"Tests","role":"test","status":"own","parents":["ac"],"title_preview_length":400}}})).await?;
    let save = |kind: &str, id: &str, title: &str, parent: Value| {
        json!({"op":"save_object","kind":kind,"id":id,"revision":0,
        "parent":parent,"data":{"title":title,"owner":"owner","body":"The rule"},"reason":"Create"})
    };
    write(save("requirement", "r", "R", Value::Null)).await?;
    write(save("ac", "a", "A", json!("requirement:r"))).await?;
    write(save("test", "t", "tests/x.rs > one", json!("ac:a"))).await?;
    let before = statuses(&pool, &ws).await?;
    let conn = pool.get().await?;
    // Only the status projection is stale: an older version and one lost rollup.
    conn.execute("UPDATE joppa.board_cache SET first_view=first_view||'{\"status_projection\":{\"version\":0,\"position\":0}}'::jsonb WHERE workspace=$1", &[&ws]).await?;
    conn.execute(
        "UPDATE joppa.objects SET rollup=NULL WHERE workspace=$1 AND address='ac:a'",
        &[&ws],
    )
    .await?;
    ensure!(
        counted(store::backfill_board(&pool)).await? == (0, 0),
        "the start waited for the status projection"
    );
    ensure!(
        counted(store::catch_up_statuses(&pool, false)).await? == (0, 1),
        "the catch-up replayed the journal"
    );
    ensure!(
        statuses(&pool, &ws).await? == before,
        "the rebuild changed a status"
    );
    ensure!(
        counted(store::catch_up_statuses(&pool, false)).await? == (0, 0),
        "a second catch-up rebuilt again"
    );
    // The daily run rebuilds each workspace from its rows and changes no status.
    let (replays, rebuilds) = counted(store::catch_up_statuses(&pool, true)).await?;
    ensure!(replays == 0 && rebuilds >= 1, "{replays} {rebuilds}");
    ensure!(statuses(&pool, &ws).await? == before);
    // Stale board rows replay the journal once at the start and give the same statuses.
    conn.execute("UPDATE joppa.board_cache SET first_view=first_view||'{\"projection_version\":\"0\"}'::jsonb WHERE workspace=$1", &[&ws]).await?;
    ensure!(
        counted(store::backfill_board(&pool)).await? == (1, 1),
        "a board rebuild did not replay"
    );
    ensure!(statuses(&pool, &ws).await? == before);
    Ok(())
}
