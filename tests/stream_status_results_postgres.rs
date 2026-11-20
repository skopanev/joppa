//! Black-box check of streamed test results, commit pruning, detail reads, and peak RSS (ac:8cidhylh).
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};

fn linux_rss_kb() -> Option<i64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    text.lines().find_map(|l| {
        l.strip_prefix("VmRSS:")?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

async fn read_status(pool: &deadpool_postgres::Pool, ws: &str, address: &str) -> Result<Value> {
    let row = pool
        .get()
        .await?
        .query_one(
            "SELECT test_status::text FROM joppa.objects WHERE workspace=$1 AND address=$2",
            &[&ws, &address],
        )
        .await
        .context("missing object row")?;
    let raw: Option<String> = row.get(0);
    Ok(serde_json::from_str(&raw.unwrap_or_else(|| "{}".into()))?)
}

// ac:8cidhylh
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn status_projection_streams_results_prunes_old_commits_and_bounds_peak_rss() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    store::backfill_board(&pool).await?;

    let ws = format!("stream-results-{}", uuid::Uuid::new_v4().simple());
    let op = serde_json::from_value(json!({"op":"workspace","title":"Stream status"}))?;
    store::execute(&pool, &ws, "owner", "ws-create", &op).await?;
    let write = async |req: &str, command: Value| {
        store::execute_remote_input(&pool, &ws, "owner", req, &command).await
    };
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    write(
        "schema-1",
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Configure dimensions and failure_field",
            "filters": ["env"],
            "test_results": {
                "statuses": {"pass": "pass", "fail": "fail", "skip": "skip", "todo": "todo"},
                "green_if_all": ["pass"], "red_if_any": ["fail"],
                "same_commit": "fail_wins", "reset_on_change": true,
                "max_tests": 200, "max_results": 1000, "failure_field": "note",
                "fields": [
                    {"name": "note", "title": "Failure note", "type": "text", "max_length": 8192},
                    {"name": "blob", "title": "Payload", "type": "text", "max_length": 8192},
                    {"name": "env", "title": "Environment", "type": "enum", "values": ["dev", "prod"]}
                ]
            },
            "types": {
                "requirement": {"title": "Requirements", "role": "requirement", "status": "children", "root": true, "parents": []},
                "ac": {"title": "Criteria", "role": "ac", "status": "children", "parents": ["requirement"], "fields": {"env": {"type": "list", "values": ["dev", "prod"]}}},
                "task": {"title": "Tasks", "role": "task", "status": "none", "parents": ["ac"]},
                "test": {"title": "Tests", "role": "test", "status": "own", "parents": ["ac"], "title_preview_length": 400}
            }
        }),
    )
    .await?;

    write("r1", json!({"op":"save_object","kind":"requirement","id":"r1","revision":0,"parent":null,"data":{"title":"Req","owner":"owner","body":"Body"},"reason":"Seed"})).await?;
    write("c1", json!({"op":"save_object","kind":"ac","id":"c","revision":0,"parent":"requirement:r1","data":{"title":"Criterion C","owner":"owner","body":"Body","env":["dev","prod"]},"reason":"Seed"})).await?;
    for (id, title) in [
        ("t1", "Old t1 title"),
        ("t2", "Scenario 2"),
        ("t3", "Scenario 3"),
    ] {
        write(id, json!({"op":"save_object","kind":"test","id":id,"revision":0,"parent":"ac:c","data":{"title":title,"owner":"owner","body":title},"reason":"Seed"})).await?;
    }

    store::record_results(
        &pool,
        &ws,
        "owner",
        &json!({
            "commit": format!("{:040x}", 1),
            "results": [
                {"test": "Old t1 title", "status": "pass", "env": "dev", "blob": "b".repeat(6000)},
                {"test": "test:t2", "status": "fail", "note": "old failure before reset"},
                {"test": "test:t3", "status": "pass"}
            ]
        }),
    )
    .await?;
    write("t1-ren", json!({"op":"save_object","kind":"test","id":"t1","revision":1,"data":{"title":"Renamed t1 title"},"reason":"Rename"})).await?;

    // Seed 596 historical records (~11 MB of messages and fields in DB) plus one non-recorded skip report.
    {
        let mut conn = pool.get().await?;
        conn.execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements", &[])
            .await?;
        let tx = conn.transaction().await?;
        let pad = "x".repeat(6000);
        for n in 2..=597i64 {
            let (sha, stamp) = (format!("{n:040x}"), format!("hist-{n}"));
            let rep_id: i64 = tx.query_one(
                "INSERT INTO joppa.test_reports(workspace,commit_sha,commit_order,branch,default_branch,dirty,full_run,uploader,stamp_hash,junit,recorded) VALUES($1,$2,$3,'record',true,false,false,'owner',$4,'',true) RETURNING id",
                &[&ws, &sha, &n, &stamp],
            ).await?.get(0);
            let fail = n % 50 == 0;
            let st = if fail { "fail" } else { "pass" };
            let msg: Option<&str> = (fail && n % 100 == 0).then_some("column failure");
            let env = if n % 2 == 0 { "dev" } else { "prod" };
            let fields = match fail && msg.is_none() {
                true => {
                    json!({"note": format!("field failure {n} {pad}"), "env": env, "blob": pad})
                }
                false => json!({"env": env, "blob": pad}),
            }
            .to_string();
            for addr in ["test:t1", "test:t2", "test:t3"] {
                tx.execute(
                    "INSERT INTO joppa.test_results(report_id,address,status,message,status_name,fields) VALUES($1,$2,$3,$4,$3,$5::text::jsonb)",
                    &[&rep_id, &addr, &st, &msg, &fields],
                ).await?;
            }
        }
        let unrec: i64 = tx.query_one(
            "INSERT INTO joppa.test_reports(workspace,commit_sha,commit_order,branch,default_branch,dirty,full_run,uploader,stamp_hash,junit,recorded) VALUES($1,$2,598,'main',true,false,false,'ci','unrec','',false) RETURNING id",
            &[&ws, &format!("{:040x}", 598)],
        ).await?.get(0);
        tx.execute("INSERT INTO joppa.test_results(report_id,address,status,message) VALUES($1,'test:t1','skipped',NULL),($1,'test:t2','skipped',NULL),($1,'test:t3','skipped',NULL)", &[&unrec]).await?;
        tx.commit().await?;
    }

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    write("t2-rst", json!({"op":"save_object","kind":"test","id":"t2","revision":1,"data":{"body":"Scenario 2 updated"},"reason":"Reset"})).await?;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let final_sha = format!("{:040x}", 600);
    store::record_results(
        &pool,
        &ws,
        "owner",
        &json!({
            "commit": final_sha,
            "results": [
                {"test": "test:t1", "status": "pass", "env": "prod", "blob": "latest-t1"},
                {"test": "test:t2", "status": "pass", "env": "dev", "blob": "latest-t2"},
                {"test": "test:t3", "status": "pass", "env": "prod", "blob": "first-run-pass"}
            ]
        }),
    )
    .await?;
    store::record_results(&pool, &ws, "owner", &json!({
        "commit": final_sha,
        "results": [
            {"test": "test:t3", "status": "fail", "note": "first fail note wins", "env": "prod"},
            {"test": "test:t3", "status": "fail", "note": "second fail note loses", "env": "prod"}
        ]
    })).await?;

    let (c_rec, t1_rec) = (
        read_status(&pool, &ws, "ac:c").await?,
        read_status(&pool, &ws, "test:t1").await?,
    );
    let (t2_rec, t3_rec) = (
        read_status(&pool, &ws, "test:t2").await?,
        read_status(&pool, &ws, "test:t3").await?,
    );
    ensure!(t1_rec["status"] == "green" && t1_rec["tests"][0]["fields"]["blob"] == "latest-t1");
    ensure!(t2_rec["status"] == "green" && t3_rec["status"] == "red");
    ensure!(
        t3_rec["tests"][0]["message"] == "first fail note wins",
        "{t3_rec}"
    );

    let conn = pool.get().await?;
    write("k1-delta", json!({"op":"save_object","kind":"task","id":"k1","revision":0,"parent":"ac:c","data":{"title":"Task under C","owner":"owner","body":"Work"},"reason":"Delta"})).await?;
    ensure!(
        read_status(&pool, &ws, "ac:c").await? == c_rec,
        "write delta changed ac:c"
    );

    let total_rows: i64 = conn.query_one("SELECT count(*) FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1", &[&ws]).await?.get(0);
    ensure!(total_rows >= 1796, "expected >=1796 rows, got {total_rows}");
    store::catch_up_statuses(&pool, false).await?;
    conn.execute("UPDATE joppa.board_cache SET first_view = jsonb_set(first_view, '{status_projection,position}', '0'::jsonb) WHERE workspace=$1", &[&ws]).await?;
    conn.execute("SELECT pg_stat_statements_reset()", &[])
        .await?;
    let rss_before = linux_rss_kb();
    ensure!(store::catch_up_statuses(&pool, false).await? == 1);
    if let (Some(before), Some(after)) = (rss_before, linux_rss_kb()) {
        let delta_kb = after - before;
        println!("VmRSS before={before} kB, after={after} kB, delta={delta_kb} kB");
        ensure!(
            delta_kb < 8192,
            "full rebuild grew VmRSS by {delta_kb} kB (>= 8 MB) for 11 MB history"
        );
    }
    let c_reb = read_status(&pool, &ws, "ac:c").await?;
    let t1_reb = read_status(&pool, &ws, "test:t1").await?;
    ensure!(c_reb == c_rec, "ac:c rebuild={c_reb} vs rec={c_rec}");
    ensure!(t1_reb == t1_rec, "t1 rebuild={t1_reb} vs rec={t1_rec}");
    ensure!(
        read_status(&pool, &ws, "test:t2").await? == t2_rec
            && read_status(&pool, &ws, "test:t3").await? == t3_rec
    );

    let stream_sql: String = conn.query_one("SELECT query FROM pg_stat_statements WHERE query LIKE '%FROM joppa.test_results%ORDER BY t.report_id DESC%' LIMIT 1", &[]).await?.get(0);
    ensure!(
        !stream_sql.contains("message") && !stream_sql.contains("duration_ms"),
        "{stream_sql}"
    );
    let detail_rows: i64 = conn.query_one("SELECT COALESCE(sum(rows)::bigint, 0) FROM pg_stat_statements WHERE query LIKE '%SELECT t.report_id,t.address,t.status,t.message,t.fields::text FROM joppa.test_results%'", &[]).await?.get(0);
    println!("detail_rows={detail_rows} out of total_rows={total_rows}");
    ensure!(
        detail_rows > 0 && detail_rows <= 20,
        "details read {detail_rows} rows"
    );
    Ok(())
}
