//! The status projection does the work on write: a read makes no status, rollup or wait
//! computation, each write transaction makes exactly one rebuild, and a missing, stale or
//! partial projection is an explicit error for every public reader, the Task queue included.
//! A list reads the stored decoration of its own rows only.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};

#[path = "parity_support/mod.rs"]
mod parity_support;
use parity_support::*;
// The readers use the probe counters, so only this target includes them, and the parity
// support stays the same on the builds before and after the change.
#[path = "parity_support/readers.rs"]
mod readers;
use readers::*;

#[tokio::test]
#[ignore = "requires an isolated joppa_test_* PostgreSQL"]
async fn reads_compute_nothing_and_each_write_rebuilds_once() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test_"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    use_workspace("counters");
    let mut n = fixture(&pool).await?;
    upload(
        &pool,
        "c1c1c1c",
        1,
        true,
        &[
            ("x1", "pass"),
            ("x2", "pass"),
            ("x3", "pass"),
            ("y1", "fail"),
            ("z1", "pass"),
        ],
    )
    .await?;
    let r1 = revision(&pool, "requirement:r1").await?;
    run(&pool, &mut n, json!({"op":"save_object","kind":"requirement","id":"r1","revision":r1,"parent":"capability:c","data":{"title":"First","owner":"owner","body":"Rule one.","consumers":["user"]},"links":[{"target":"requirement:r2","relation":"depends_on","blocking":true}],"reason":"Order after r2"})).await?;

    // Reads: no status, rollup or wait computation.
    let before = counts();
    for result in reads(&pool).await {
        result?;
    }
    let [status, rollup, waits, _, rebuild] = delta(before);
    ensure!(
        (status, rollup, waits, rebuild) == (0, 0, 0, 0),
        "reads computed status={status} rollup={rollup} waits={waits} rebuild={rebuild}"
    );

    // A command, an upload, a repeat receipt and a batch of three: one rebuild each.
    let before = counts();
    run(&pool, &mut n, json!({"op":"set_ac_tests","target":"ac:a2","tests":list(&["y1"]),"reason":"Save the same list"})).await?;
    ensure!(
        delta(before)[4] == 1,
        "a command rebuilds once: {:?}",
        delta(before)
    );
    let before = counts();
    upload(
        &pool,
        "c2c2c2c",
        2,
        true,
        &[
            ("x1", "pass"),
            ("x2", "pass"),
            ("x3", "pass"),
            ("y1", "pass"),
            ("z1", "pass"),
        ],
    )
    .await?;
    ensure!(
        delta(before)[4] == 1,
        "an upload rebuilds once: {:?}",
        delta(before)
    );
    let before = counts();
    upload(
        &pool,
        "c2c2c2c",
        2,
        true,
        &[
            ("x1", "pass"),
            ("x2", "pass"),
            ("x3", "pass"),
            ("y1", "pass"),
            ("z1", "pass"),
        ],
    )
    .await?;
    ensure!(
        delta(before)[4] == 1,
        "a repeat receipt rebuilds once: {:?}",
        delta(before)
    );
    let operations: Vec<store::BatchOperation> = ["x1", "x2", "x3"]
        .iter()
        .map(|t| serde_json::from_value(json!({"command":{"op":"set_ac_tests","target":"ac:a1","tests":list(&[t]),"reason":"Narrow the list"}})))
        .collect::<Result<_, _>>()?;
    let before = counts();
    store::execute_batch(&pool, ws(), "owner", "counters-batch", &operations).await?;
    ensure!(
        delta(before)[4] == 1,
        "a batch of three rebuilds once: {:?}",
        delta(before)
    );

    // A missing, older-version or older-position projection is refused by every reader.
    for broken in [
        Value::Null,
        json!({"version":0,"position":0}),
        json!({"version":1,"position":-1}),
    ] {
        set_projection(&pool, broken.clone()).await?;
        for (index, result) in reads(&pool).await.into_iter().enumerate() {
            let error = result
                .err()
                .ok_or_else(|| anyhow::anyhow!("reader {index} read a {broken} projection"))?;
            ensure!(
                error.is::<store::ProjectionNotReady>(),
                "reader {index}: {error:#}"
            );
        }
    }
    // The next write rebuilds the projection, and the readers work again.
    run(&pool, &mut n, json!({"op":"set_ac_tests","target":"ac:a2","tests":list(&["y1"]),"reason":"Save the same list again"})).await?;
    for result in reads(&pool).await {
        result?;
    }

    // The domain list reads only its own rows; the Task queue reads the stored waits once.
    let domains = store::ObjectScope {
        kind: Some("domain"),
        limit: 50,
        ..Default::default()
    };
    let before = counts();
    store::read_objects(&pool, ws(), "owner", &domains).await?;
    ensure!(
        delta(before)[3] == 0,
        "the domain list read the waits of the workspace"
    );
    let before = counts();
    store::task_queue(&pool, ws(), "owner", "all", 100, None).await?;
    ensure!(
        delta(before)[3] == 1,
        "the queue reads the stored waits once: {:?}",
        delta(before)
    );

    // A lost row in a valid projection is refused by every reader; backfill_board repairs it.
    for column in ["test_status", "rollup"] {
        let lost = pool
            .get()
            .await?
            .execute(
                &format!(
                    "UPDATE joppa.objects SET {column}=NULL WHERE workspace=$1 AND address='ac:a1'"
                ),
                &[&ws()],
            )
            .await?;
        ensure!(lost == 1, "ac:a1 must exist");
        for (index, result) in reads(&pool).await.into_iter().enumerate() {
            let error = result.err().ok_or_else(|| {
                anyhow::anyhow!("reader {index} read a projection without {column}")
            })?;
            ensure!(
                error.is::<store::ProjectionNotReady>(),
                "reader {index}: {error:#}"
            );
        }
        store::backfill_board(&pool).await?;
        let gaps: i64 = pool.get().await?.query_one(
            "SELECT count(*) FROM joppa.objects WHERE workspace=$1 AND (test_status IS NULL OR rollup IS NULL)",
            &[&ws()],
        ).await?.get(0);
        ensure!(
            gaps == 0,
            "backfill left {gaps} rows without a stored value"
        );
        for result in reads(&pool).await {
            result?;
        }
    }
    Ok(())
}
