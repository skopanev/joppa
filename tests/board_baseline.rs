//! Synthetic-fixture baseline of the board top-level read. It changes no product code.
//! The fixture has the shape of joppa, scaled past 700 objects: 5 domains, 25 capabilities,
//! 76 requirements with 13 links, 202 criteria with 2 listed tests each, and 420 Tasks.
//! 40 clean full reports cover every listed test. The test times 20 reads of the top
//! level as the board makes them (kind=domain, limit=50) and prints the SQL statements
//! by total time. It runs only on a fresh isolated joppa_test_* database.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::time::Instant;

#[path = "baseline_support/mod.rs"]
mod baseline_support;
use baseline_support::*;

#[tokio::test]
#[ignore = "requires an isolated joppa_test_* PostgreSQL with pg_stat_statements preloaded"]
async fn board_top_level_baseline_on_a_synthetic_fixture() -> Result<()> {
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
    pool.get()
        .await?
        .batch_execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
        .await?;
    store::migrate(&pool).await?;
    let ws = format!("baseline-{}", uuid::Uuid::new_v4().simple());
    build(&pool, &ws).await?;
    let sample = store::criterion_tests(&pool, &ws, "owner", "ac:a0").await?;
    ensure!(
        sample["tests"].as_array().is_some_and(|t| t.len() == 2),
        "the listed tests resolve: {sample}"
    );
    let scope = store::ObjectScope {
        kind: Some("domain"),
        limit: 50,
        ..Default::default()
    };
    for _ in 0..3 {
        store::read_objects(&pool, &ws, "owner", &scope).await?;
    }
    pool.get().await?.query_one(
        "SELECT pg_stat_statements_reset(0,(SELECT oid FROM pg_database WHERE datname=current_database()),0)", &[]).await?;
    let mut times = Vec::with_capacity(20);
    for _ in 0..20 {
        let begin = Instant::now();
        let page = store::read_objects(&pool, &ws, "owner", &scope).await?;
        times.push(begin.elapsed().as_micros() as f64 / 1000.0);
        ensure!(
            page["items"].as_array().is_some_and(|i| i.len() == 5),
            "five domains: {page}"
        );
    }
    // Write latency: 20 single commands and 20 uploads, each its own transaction.
    let mut command_ms = vec![];
    for i in 0..20 {
        let tests = [
            format!("{FILE} > {}", test_name(i, 0)),
            format!("{FILE} > {}", test_name(i, 1)),
        ];
        let begin = Instant::now();
        store::execute_input(&pool, &ws, "owner", &format!("timing-{i}"), &json!({"op":"set_ac_tests","target":format!("ac:a{i}"),"tests":tests,"reason":"Save the same list"})).await?;
        command_ms.push(begin.elapsed().as_micros() as f64 / 1000.0);
    }
    let mut upload_ms = vec![];
    for report in REPORTS..REPORTS + 20 {
        let stamp = json!({"commit":format!("{:040x}", SEED as u128 * 1_000 + report as u128),"order":report + 1,
            "branch":"master","default_branch":true,"dirty":false,"full":true});
        let begin = Instant::now();
        store::upload_report(
            &pool,
            &ws,
            "owner",
            serde_json::from_value(json!({"stamp":stamp,"junit":junit(report)}))?,
        )
        .await?;
        upload_ms.push(begin.elapsed().as_micros() as f64 / 1000.0);
    }
    let p95 = |values: &[f64]| {
        let mut v = values.to_vec();
        v.sort_by(|a, b| a.total_cmp(b));
        v[18]
    };
    println!(
        "{}",
        json!({"label":"synthetic-fixture write latency","command_ms":command_ms,"command_p95_ms":p95(&command_ms),"upload_ms":upload_ms,"upload_p95_ms":p95(&upload_ms)})
    );
    let statements = pool.get().await?.query(
        "SELECT left(regexp_replace(query,'\\s+',' ','g'),140),calls,round(total_exec_time::numeric,1)::float8 FROM pg_stat_statements
         WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database()) ORDER BY total_exec_time DESC LIMIT 12", &[]).await?;
    let sql_ms: f64 = statements.iter().map(|r| r.get::<_, f64>(2)).sum();
    let top: Vec<Value> = statements.iter().map(|r| json!({"query":r.get::<_,String>(0),"calls":r.get::<_,i64>(1),"total_ms":r.get::<_,f64>(2)})).collect();
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let counts = json!({
        "objects": count(&pool, "SELECT count(*) FROM joppa.objects WHERE workspace=$1 AND present", &ws).await?,
        "links": count(&pool, "SELECT count(*) FROM joppa.object_links WHERE workspace=$1", &ws).await?,
        "reports": count(&pool, "SELECT count(*) FROM joppa.test_reports WHERE workspace=$1", &ws).await?,
        "results": count(&pool, "SELECT count(*) FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1", &ws).await?,
    });
    println!(
        "{}",
        json!({"label":"synthetic-fixture baseline, not a measured production cause","seed":SEED,
        "counts":counts,"read_ms":times,"p95_ms":sorted[18],"sql_top_scope":"mixed: 20 reads + 20 commands + 20 uploads, not read-only cost",
        "sql_top_total_ms_mixed":sql_ms,"sql_top":top})
    );
    Ok(())
}
