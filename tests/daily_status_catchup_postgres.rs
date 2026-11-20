//! Black-box check of startup and daily status catch-up (ac:h3apxyfi).
use anyhow::{Context, Result, ensure};
use joppa::{probe, store};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

async fn seed_workspace(pool: &deadpool_postgres::Pool, label: &str) -> Result<String> {
    let ws = format!("daily-catchup-{label}-{}", uuid::Uuid::new_v4().simple());
    let op = serde_json::from_value(json!({"op":"workspace","title":label}))?;
    store::execute(pool, &ws, "owner", &format!("ws-{label}"), &op).await?;
    let write = async |req: &str, command: Value| {
        store::execute_remote_input(pool, &ws, "owner", req, &command).await
    };
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    write(
        "schema",
        json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
            "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
            "ac":{"title":"Criteria","role":"ac","status":"children","parents":["requirement"]},
            "test":{"title":"Tests","role":"test","status":"own","parents":["ac"],"title_preview_length":400}
        }}),
    )
    .await?;
    let save = |kind: &str, id: &str, parent: Value| {
        json!({"op":"save_object","kind":kind,"id":id,"revision":0,
            "parent":parent,"data":{"title":id,"owner":"owner","body":"Rule text"},"reason":"Seed"})
    };
    write("r1", save("requirement", "r1", Value::Null)).await?;
    write("a1", save("ac", "a1", json!("requirement:r1"))).await?;
    write("t1", save("test", "t1", json!("ac:a1"))).await?;
    Ok(ws)
}

async fn stamp_row(pool: &deadpool_postgres::Pool, ws: &str) -> Result<(String, i64, i64)> {
    let row = pool
        .get()
        .await?
        .query_one(
            "SELECT xmin::text, COALESCE((first_view->'status_projection'->>'position')::bigint, -1), position FROM joppa.board_cache WHERE workspace=$1",
            &[&ws],
        )
        .await
        .context("missing board_cache row")?;
    Ok((row.get(0), row.get(1), row.get(2)))
}

async fn make_stale(pool: &deadpool_postgres::Pool, ws: &str) -> Result<String> {
    pool.get()
        .await?
        .execute(
            "UPDATE joppa.board_cache SET first_view = jsonb_set(first_view, '{status_projection,position}', '0'::jsonb) WHERE workspace=$1",
            &[&ws],
        )
        .await?;
    Ok(stamp_row(pool, ws).await?.0)
}

async fn wait_caught_up(pool: &deadpool_postgres::Pool, ws: &str) -> Result<(String, i64)> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (xmin, stamped_pos, board_pos) = stamp_row(pool, ws).await?;
        if stamped_pos == board_pos && board_pos > 0 {
            return Ok((xmin, stamped_pos));
        }
        ensure!(
            Instant::now() < deadline,
            "{ws} stayed stale at {stamped_pos}/{board_pos}"
        );
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}

// ac:h3apxyfi
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn daily_status_catch_up_rebuilds_only_the_stale_workspaces() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    store::backfill_board(&pool).await?;
    let w1 = seed_workspace(&pool, "w1").await?;
    let w2 = seed_workspace(&pool, "w2").await?;
    store::catch_up_statuses(&pool, false).await?;

    let (w1_xmin_before, w1_stamped, w1_pos) = stamp_row(&pool, &w1).await?;
    ensure!(w1_stamped == w1_pos && w1_pos > 0, "W1 must start current");
    let w2_stale_xmin_1 = make_stale(&pool, &w2).await?;

    let rebuilds_0 = probe::read(&probe::REBUILD);
    let task = tokio::spawn(store::status_projection_loop(pool.clone()));
    let (w2_xmin_after_start, _) = wait_caught_up(&pool, &w2).await?;
    let (w1_xmin_after_start, _, _) = stamp_row(&pool, &w1).await?;
    let rebuilds_start = probe::read(&probe::REBUILD) - rebuilds_0;
    ensure!(
        w2_xmin_after_start != w2_stale_xmin_1,
        "startup catch-up must rebuild W2"
    );
    ensure!(
        w1_xmin_after_start == w1_xmin_before,
        "startup catch-up rebuilt current W1"
    );
    ensure!(
        rebuilds_start == 1,
        "startup catch-up rebuilt {rebuilds_start} workspaces instead of 1"
    );

    let w2_stale_xmin_2 = make_stale(&pool, &w2).await?;
    let rebuilds_1 = probe::read(&probe::REBUILD);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(24 * 60 * 60 + 1)).await;
    tokio::time::resume();

    let (w2_xmin_after_daily, _) = wait_caught_up(&pool, &w2).await?;
    task.abort();
    let (w1_xmin_after_daily, _, _) = stamp_row(&pool, &w1).await?;
    let rebuilds_daily = probe::read(&probe::REBUILD) - rebuilds_1;
    ensure!(
        w2_xmin_after_daily != w2_stale_xmin_2,
        "daily catch-up must rebuild stale W2"
    );
    ensure!(
        w1_xmin_after_daily == w1_xmin_before,
        "daily catch-up rebuilt current W1"
    );
    ensure!(
        rebuilds_daily == 1,
        "daily catch-up rebuilt {rebuilds_daily} workspaces instead of 1"
    );
    Ok(())
}
