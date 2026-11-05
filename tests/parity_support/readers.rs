//! The readers and the probe counts of the projection counter test. Only that target
//! includes this file, as a child module, so `super` is the test root.
use super::*;
use joppa::probe;

/// STATUS, ROLLUP, WAITS, WAIT_READS and REBUILD.
pub fn counts() -> [u64; 6] {
    [
        &probe::STATUS,
        &probe::ROLLUP,
        &probe::WAITS,
        &probe::WAIT_READS,
        &probe::REBUILD,
        &probe::DELTA,
    ]
    .map(probe::read)
}

pub fn delta(before: [u64; 6]) -> [u64; 6] {
    let after = counts();
    [0, 1, 2, 3, 4, 5].map(|i| after[i] - before[i])
}

/// Every reader that the board, the tests view and the agents use.
pub async fn reads(pool: &deadpool_postgres::Pool) -> Vec<Result<Value>> {
    let scopes = [
        store::ObjectScope {
            filter: joppa::node_filter::NodeFilter::of(None, Some("domain")),
            limit: 50,
            ..Default::default()
        },
        store::ObjectScope {
            filter: joppa::node_filter::NodeFilter::of(Some("ac:a1"), None),
            ..Default::default()
        },
        store::ObjectScope {
            filter: joppa::node_filter::NodeFilter::of(Some("*"), None),
            limit: 100,
            ..Default::default()
        },
        store::ObjectScope {
            filter: joppa::node_filter::NodeFilter::parse(
                &serde_json::json!({"kind":"task","ready":true}),
            )
            .unwrap(),
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
    out.push(store::test_status(pool, ws(), "owner", "ac:a1").await);
    out
}

pub async fn set_projection(pool: &deadpool_postgres::Pool, value: Value) -> Result<()> {
    pool.get().await?.execute(
        "UPDATE joppa.board_cache SET first_view=CASE WHEN $2::text::jsonb='null'::jsonb THEN first_view-'status_projection' ELSE jsonb_set(first_view,'{status_projection}',$2::text::jsonb) END WHERE workspace=$1",
        &[&ws(), &value.to_string()],
    ).await?;
    Ok(())
}
