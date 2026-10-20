use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::time::Instant;

fn p95(values: &mut [u128]) -> u128 {
    values.sort_unstable();
    values[values.len().div_ceil(20) * 19 - 1]
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn current_object_read_has_no_journal_select_and_bounded_history_latency() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let conn = pool.get().await?;
    let database: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    conn.batch_execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
        .await?;
    drop(conn);
    store::migrate(&pool).await?;
    let workspace = format!("read-latency-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &workspace,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Read latency"}),
    )
    .await?;
    store::execute_input(
        &pool,
        &workspace,
        "owner",
        "agent",
        &json!({"op":"enroll_service","id":"bot","role":"agent"}),
    )
    .await?;

    for (id, events) in [("short", 10), ("long", 1000)] {
        for revision in 0..events {
            let command = json!({"op":"save_object","kind":"note","id":id,"revision":revision,"parent":null,
                "data":{"title":"Measured object","owner":"owner","body":format!("Source revision {revision:04}")},
                "reason":"Record source revision"});
            store::execute_remote_input(
                &pool,
                &workspace,
                "bot",
                &format!("{id}-{revision}"),
                &command,
            )
            .await?;
        }
        let count: i64 = pool.get().await?.query_one(
            "SELECT count(*) FROM joppa.events WHERE workspace=$1 AND command->>'op'='save_object' AND command->>'id'=$2",
            &[&workspace, &id],
        ).await?.get(0);
        assert_eq!(count, events as i64);
    }
    let short = store::ObjectScope {
        address: Some("note:short"),
        ..Default::default()
    };
    let long = store::ObjectScope {
        address: Some("note:long"),
        ..Default::default()
    };
    for _ in 0..10 {
        store::read_objects(&pool, &workspace, "bot", &short).await?;
        store::read_objects(&pool, &workspace, "bot", &long).await?;
    }
    pool.get().await?.query_one(
        "SELECT pg_stat_statements_reset(0,(SELECT oid FROM pg_database WHERE datname=current_database()),0)",
        &[],
    ).await?;
    let mut short_times = Vec::with_capacity(200);
    let mut long_times = Vec::with_capacity(200);
    for index in 0..200 {
        let order = if index % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        };
        for is_long in order {
            let begin = Instant::now();
            let result = store::read_objects(
                &pool,
                &workspace,
                "bot",
                if is_long { &long } else { &short },
            )
            .await?;
            let elapsed = begin.elapsed().as_micros();
            assert_eq!(result["current_revision"], if is_long { 1000 } else { 10 });
            if is_long {
                long_times.push(elapsed);
            } else {
                short_times.push(elapsed);
            }
        }
    }
    let statements = pool.get().await?.query(
        "SELECT query,calls FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND userid=(SELECT usesysid FROM pg_user WHERE usename=current_user)",
        &[],
    ).await?;
    let journal_selects: Vec<Value> = statements
        .into_iter()
        .filter_map(|row| {
            let query: String = row.get(0);
            let normal = query
                .to_ascii_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            (normal.contains("from joppa.events") || normal.contains("join joppa.events"))
                .then(|| json!({"query":query,"calls":row.get::<_,i64>(1)}))
        })
        .collect();
    let short_p95 = p95(&mut short_times);
    let long_p95 = p95(&mut long_times);
    let ratio = long_p95 as f64 / short_p95 as f64;
    println!(
        "{}",
        json!({"reads_per_object":200,"short_events":10,"long_events":1000,
        "short_p95_us":short_p95,"long_p95_us":long_p95,"ratio":ratio,"journal_selects":journal_selects})
    );
    assert!(
        journal_selects.is_empty(),
        "current reads selected journal events"
    );
    assert!(ratio <= 1.5, "long history p95 ratio {ratio} exceeds 1.5");
    Ok(())
}
