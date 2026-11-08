//! Reads of a workspace answer while its writers wait for its advisory lock, and the waiting
//! writers hold at most 1 connection of the server pool.
#[path = "common/pg_plan.rs"]
mod pg_plan;
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// The connections of the pool that a caller holds now.
fn in_use(pool: &pg_plan::Pool) -> Value {
    let s = pool.status();
    json!({"max_size":s.max_size,"size":s.size,"available":s.available,"waiting":s.waiting,
        "in_use":s.size.saturating_sub(s.available)})
}

/// The sessions that wait for an advisory lock now.
async fn lock_waits(pool: &pg_plan::Pool) -> Result<i64> {
    let sql = "SELECT count(*) FROM pg_locks WHERE locktype='advisory' AND NOT granted AND database=(SELECT oid FROM pg_database WHERE datname=current_database())";
    Ok(pool.get().await?.query_one(sql, &[]).await?.get(0))
}

// ac:voa75erx
// test:zrlwyh2g
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn reads_answer_within_a_second_while_five_writers_wait_for_the_workspace() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = pg_plan::db().await?;
    let ws = pg_plan::workspace(&pool, "write-queue").await?;
    pg_plan::many_nodes(&pool, &ws, 200, 10).await?;
    // The test connection is outside the pool. It holds the advisory lock of the workspace.
    let (holder, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await?;
    tokio::spawn(connection);
    holder
        .execute(
            "SELECT pg_advisory_lock(hashtextextended($1,91827102))",
            &[&ws],
        )
        .await?;
    let writers: Vec<_> = (0..5)
        .map(|i| {
            let (pool, ws) = (pool.clone(), ws.clone());
            tokio::spawn(async move {
                let save = json!({"op":"save_object","kind":"domain","id":format!("w{i}"),"revision":0,
                    "parent":null,"data":{"title":format!("Written {i}"),"owner":"owner"},"reason":"Create"});
                let request = format!("queue-{i}");
                store::execute_remote_input(&pool, &ws, "owner", &request, &save).await
            })
        })
        .collect();
    // Wait until a writer waits for the lock in the database.
    let start = Instant::now();
    while lock_waits(&pool).await? == 0 {
        ensure!(start.elapsed() < Duration::from_secs(10), "no writer waits");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let waiting = in_use(&pool);
    let mut reads = vec![];
    for i in 0..20 {
        let start = Instant::now();
        if i % 2 == 0 {
            store::board_read(&pool, &ws, "bot").await?;
        } else {
            let scope = store::ObjectScope {
                address: Some("domain:d7"),
                ..Default::default()
            };
            store::read_objects(&pool, &ws, "bot", &scope).await?;
        }
        reads.push(start.elapsed().as_secs_f64());
    }
    let after_reads = in_use(&pool);
    let finished = writers.iter().filter(|w| w.is_finished()).count();
    let lock_waiters = lock_waits(&pool).await?;
    holder
        .execute(
            "SELECT pg_advisory_unlock(hashtextextended($1,91827102))",
            &[&ws],
        )
        .await?;
    let mut replies = vec![];
    for writer in writers {
        let reply = tokio::time::timeout(Duration::from_secs(30), writer)
            .await
            .context("a write did not end after the lock was released")??;
        replies.push(reply.map_err(|e| anyhow::anyhow!("{e:#}"))?);
    }
    let evidence = json!({"read_seconds":reads,"pool_while_waiting":waiting,
        "pool_after_reads":after_reads,"lock_waiters":lock_waiters,"replies":replies});
    println!("{evidence}");
    ensure!(
        finished == 0,
        "{finished} writes ended before the release: {evidence}"
    );
    ensure!(reads.iter().all(|s| *s < 1.0), "a slow read: {evidence}");
    for status in [&waiting, &after_reads] {
        ensure!(status["max_size"] == 32, "{evidence}");
        ensure!(
            status["in_use"].as_u64() <= Some(1),
            "the waiting writes hold more than 1 connection: {evidence}"
        );
    }
    ensure!(lock_waiters == 1, "{evidence}");
    let mut positions: Vec<u64> = replies
        .iter()
        .filter_map(|r| r["position"].as_u64())
        .collect();
    positions.sort_unstable();
    positions.dedup();
    ensure!(positions.len() == 5, "{evidence}");
    for i in 0..5 {
        let scope = store::ObjectScope {
            address: Some(&format!("domain:w{i}")),
            ..Default::default()
        };
        store::read_objects(&pool, &ws, "bot", &scope).await?;
    }
    Ok(())
}
