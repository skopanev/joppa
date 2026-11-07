//! The status projection does the work on write: a read makes no status, rollup or wait
//! computation, each write transaction makes exactly one partial projection, and a missing, stale or
//! partial projection keeps every public reader on the stored rows until the catch-up.
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
async fn reads_compute_nothing_and_each_write_projects_once() -> Result<()> {
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
    record(
        &pool,
        "c1c1c1c",
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
    let [status, rollup, waits, _, rebuild, partial] = delta(before);
    ensure!(
        (status, rollup, waits, rebuild, partial) == (0, 0, 0, 0, 0),
        "reads computed status={status} rollup={rollup} waits={waits} rebuild={rebuild} partial={partial}"
    );

    // A command, a record, a repeat receipt and a batch of three: one partial projection each.
    let before = counts();
    run(
        &pool,
        &mut n,
        edit_r2(&pool, "Rule two, first edit.").await?,
    )
    .await?;
    ensure!(
        delta(before)[4..] == [0, 1],
        "a command projects once and partially: {:?}",
        delta(before)
    );
    let before = counts();
    record(
        &pool,
        "c2c2c2c",
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
        delta(before)[4..] == [0, 1],
        "a record projects once and partially: {:?}",
        delta(before)
    );
    let before = counts();
    record(
        &pool,
        "c2c2c2c",
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
        delta(before)[4..] == [0, 1],
        "a repeat receipt projects once and partially: {:?}",
        delta(before)
    );
    let operations: Vec<store::BatchOperation> = ["y2", "y3", "y4"]
        .iter()
        .map(|t| serde_json::from_value(json!({ "command": bind("ac:a2", t) })))
        .collect::<Result<_, _>>()?;
    let before = counts();
    store::execute_batch(&pool, ws(), "owner", "counters-batch", &operations).await?;
    ensure!(
        delta(before)[4..] == [0, 1],
        "a batch of three projects once and partially: {:?}",
        delta(before)
    );

    // A missing, older-version or older-position projection keeps every reader working on
    // the stored rows, and the catch-up after the start rebuilds it once.
    let stored = reads(&pool).await.into_iter().collect::<Result<Vec<_>>>()?;
    for broken in [
        Value::Null,
        json!({"version":0,"position":0}),
        json!({"version":1,"position":-1}),
    ] {
        set_projection(&pool, broken.clone()).await?;
        let before = counts();
        let kept = reads(&pool).await.into_iter().collect::<Result<Vec<_>>>()?;
        ensure!(kept == stored, "a {broken} projection changed a read");
        ensure!(
            delta(before) == [0; 6],
            "a read computed: {:?}",
            delta(before)
        );
        store::catch_up_statuses(&pool, false).await?;
        ensure!(
            delta(before)[4] == 1,
            "the catch-up rebuilt {:?}",
            delta(before)
        );
    }
    // The next write rebuilds the projection, and the readers work again.
    run(
        &pool,
        &mut n,
        edit_r2(&pool, "Rule two, second edit.").await?,
    )
    .await?;
    for result in reads(&pool).await {
        result?;
    }

    // The domain list reads only its own rows.
    let domains = store::ObjectScope {
        filter: joppa::node_filter::NodeFilter::of(None, Some("domain")),
        limit: 50,
        ..Default::default()
    };
    let before = counts();
    store::read_objects(&pool, ws(), "owner", &domains).await?;
    ensure!(
        delta(before)[3] == 0,
        "the domain list read the waits of the workspace"
    );

    // A lost row reads without decoration; the catch-up stores it again.
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
        for result in reads(&pool).await {
            result?;
        }
        store::catch_up_statuses(&pool, false).await?;
        let gaps: i64 = pool.get().await?.query_one(
            "SELECT count(*) FROM joppa.objects WHERE workspace=$1 AND (test_status IS NULL OR rollup IS NULL)",
            &[&ws()],
        ).await?.get(0);
        ensure!(
            gaps == 0,
            "the catch-up left {gaps} rows without a stored value"
        );
    }
    Ok(())
}

/// A save of requirement:r2 with a new body at its current revision.
async fn edit_r2(pool: &deadpool_postgres::Pool, body: &str) -> Result<Value> {
    let revision = revision(pool, "requirement:r2").await?;
    Ok(
        json!({"op":"save_object","kind":"requirement","id":"r2","revision":revision,
        "parent":"capability:c","data":{"title":"Second","owner":"owner","body":body,
        "consumers":["user"]},"reason":"Edit the rule"}),
    )
}
