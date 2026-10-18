//! The readers and the probe counts of the projection counter test. Only that target
//! includes this file, as a child module, so `super` is the test root.
use super::*;
use joppa::probe;

/// STATUS, ROLLUP, WAITS, WAIT_READS and REBUILD.
pub fn counts() -> [u64; 5] {
    [
        &probe::STATUS,
        &probe::ROLLUP,
        &probe::WAITS,
        &probe::WAIT_READS,
        &probe::REBUILD,
    ]
    .map(probe::read)
}

pub fn delta(before: [u64; 5]) -> [u64; 5] {
    let after = counts();
    [0, 1, 2, 3, 4].map(|i| after[i] - before[i])
}

/// Every reader that the board, the tests view and the agents use.
pub async fn reads(pool: &deadpool_postgres::Pool) -> Vec<Result<Value>> {
    let scopes = [
        store::ObjectScope {
            kind: Some("domain"),
            limit: 50,
            ..Default::default()
        },
        store::ObjectScope {
            parent: Some("ac:a1"),
            ..Default::default()
        },
        store::ObjectScope {
            parent: Some("*"),
            limit: 100,
            ..Default::default()
        },
        store::ObjectScope {
            work_filter: Some("unstarted"),
            ..Default::default()
        },
        store::ObjectScope {
            address: Some("task:t1"),
            ..Default::default()
        },
        store::ObjectScope {
            address: Some("requirement:r1"),
            ..Default::default()
        },
    ];
    let mut out = vec![];
    for scope in &scopes {
        out.push(store::read_objects(pool, ws(), "owner", scope).await);
    }
    out.push(store::criterion_tests(pool, ws(), "owner", "ac:a1").await);
    out.push(store::task_status(pool, ws(), "owner", "task:t1").await);
    for view in ["all", "available"] {
        out.push(store::task_queue(pool, ws(), "owner", view, 100, None).await);
    }
    out
}

pub async fn set_projection(pool: &deadpool_postgres::Pool, value: Value) -> Result<()> {
    pool.get().await?.execute(
        "UPDATE joppa.board_cache SET first_view=CASE WHEN $2::text::jsonb='null'::jsonb THEN first_view-'status_projection' ELSE jsonb_set(first_view,'{status_projection}',$2::text::jsonb) END WHERE workspace=$1",
        &[&ws(), &value.to_string()],
    ).await?;
    Ok(())
}
