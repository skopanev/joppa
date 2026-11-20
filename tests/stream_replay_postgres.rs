//! Black-box check of streamed journal replay, bounded kept events, full_state without events, Scope::full, and peak RSS (ac:ak1eh8yb).
#[path = "common/pg_plan.rs"]
mod pg_plan;
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::collections::BTreeMap;

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

async fn exec_local(pool: &pg_plan::Pool, ws: &str, req: &str, cmd: Value) -> Result<Value> {
    let op: joppa::commands::Command = serde_json::from_value(cmd)?;
    store::execute(pool, ws, "owner", req, &op).await
}

type DbSnap = (
    i64,
    String,
    BTreeMap<String, (i32, String, Option<String>, Option<String>)>,
    Vec<(String, i32, String)>,
);

async fn db_snapshot(pool: &pg_plan::Pool, ws: &str) -> Result<DbSnap> {
    let conn = pool.get().await?;
    let bc = conn
        .query_one(
            "SELECT position, (first_view - 'status_projection')::text FROM joppa.board_cache WHERE workspace=$1",
            &[&ws],
        )
        .await?;
    let objs = conn
        .query(
            "SELECT address, revision, document::text, rollup::text, test_status::text
             FROM joppa.objects WHERE workspace=$1 ORDER BY address",
            &[&ws],
        )
        .await?
        .into_iter()
        .map(|r| (r.get(0), (r.get(1), r.get(2), r.get(3), r.get(4))))
        .collect();
    let revs = conn
        .query(
            "SELECT address, revision, document::text
             FROM joppa.object_revisions WHERE workspace=$1 ORDER BY address, revision",
            &[&ws],
        )
        .await?
        .into_iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect();
    Ok((bc.get(0), bc.get(1), objs, revs))
}

async fn mark_board_stale(pool: &pg_plan::Pool, ws: &str) -> Result<()> {
    pool.get()
        .await?
        .execute(
            "UPDATE joppa.board_cache SET first_view = jsonb_set(first_view, '{projection_version}', '\"0\"'::jsonb) WHERE workspace=$1",
            &[&ws],
        )
        .await?;
    Ok(())
}

// ac:ak1eh8yb
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn journal_replay_and_full_state_stream_rows_and_bound_rss() -> Result<()> {
    let pool = pg_plan::db().await?;
    let ws = pg_plan::workspace(&pool, "stream-replay").await?;
    let write = async |req: &str, cmd: Value| {
        store::execute_remote_input(&pool, &ws, "owner", req, &cmd).await
    };
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    write(
        "schema-1",
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Configure types for replay",
            "types": {
                "requirement": {"title": "Requirements", "role": "requirement", "status": "children", "root": true, "parents": []},
                "ac": {"title": "Criteria", "role": "ac", "status": "children", "parents": ["requirement"]},
                "test": {"title": "Tests", "role": "test", "status": "own", "parents": ["ac"], "title_preview_length": 400},
                "note": {"title": "Notes", "role": "document", "inbox": true, "status": "none", "root": true, "parents": [], "field_max_length": 8192}
            }
        }),
    )
    .await?;

    write("r1", json!({"op":"save_object","kind":"requirement","id":"r1","revision":0,"parent":null,"data":{"title":"R1","owner":"owner","body":"Req"},"reason":"Seed"})).await?;
    write("a1", json!({"op":"save_object","kind":"ac","id":"a1","revision":0,"parent":"requirement:r1","data":{"title":"A1","owner":"owner","body":"AC v1"},"reason":"Seed"})).await?;
    write("a1-v2", json!({"op":"save_object","kind":"ac","id":"a1","revision":1,"data":{"body":"AC v2"},"reason":"Rev 2"})).await?;
    write("t1", json!({"op":"save_object","kind":"test","id":"t1","revision":0,"parent":"ac:a1","data":{"title":"T1","owner":"owner","body":"Test"},"reason":"Seed"})).await?;
    store::record_results(
        &pool,
        &ws,
        "owner",
        &json!({"commit":format!("{:040x}", 7),"results":[{"test":"test:t1","status":"pass"}]}),
    )
    .await?;
    write("sec-1", json!({"op":"save_object","kind":"note","id":"secret","revision":0,"parent":null,"data":{"title":"Secret","owner":"owner","body":"Leak s3cr3t v1"},"reason":"Seed"})).await?;
    write("sec-2", json!({"op":"save_object","kind":"note","id":"secret","revision":1,"data":{"body":"Leak s3cr3t v2"},"reason":"Update"})).await?;
    exec_local(
        &pool,
        &ws,
        "redact-1",
        json!({"op":"redact_object","target":"note:secret","fragment":"s3cr3t","reason":"Remove token"}),
    )
    .await?;
    write(
        "last-live",
        json!({"op":"note","target":"ac:a1","id":"live-end","text":"Final live note"}),
    )
    .await?;

    // 1. Prove streamed backfill_board replay equals live incremental writes byte-for-byte.
    let snap_live = db_snapshot(&pool, &ws).await?;
    let full_live = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    mark_board_stale(&pool, &ws).await?;
    let rebuilt = store::backfill_board(&pool).await?;
    ensure!(rebuilt >= 1, "expected backfill_board to replay ws");
    let snap_replayed = db_snapshot(&pool, &ws).await?;
    let full_replayed = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    ensure!(
        snap_live == snap_replayed,
        "replayed DB rows differed from live rows"
    );
    ensure!(
        full_live == full_replayed,
        "Scope::full after replay differed from live"
    );

    // 2. Append 2 995 padded journal events (~12 MB) so ws has >= 3 000 events, ending with a note.
    let conn = pool.get().await?;
    let base_pos: i64 = conn
        .query_one(
            "SELECT max(position) FROM joppa.events WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0);
    let pad = "p".repeat(4000);
    conn.execute(
        "INSERT INTO joppa.events(workspace, position, actor, request_id, command)
         SELECT $1, $2 + g, 'owner', 'bulk-' || g,
                jsonb_build_object('op', 'note', 'target', 'ac:a1', 'id', 'n-' || g, 'text', $3 || g::text)
         FROM generate_series(1::bigint, 2995::bigint) g",
        &[&ws, &base_pos, &pad],
    )
    .await?;

    // Initial replay builds the 3 000-event state.
    store::backfill_board(&pool).await?;

    // Verify migrate_notes calls full_state (MAX(position)) without reading joppa.events rows.
    conn.execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements", &[])
        .await?;
    conn.execute(
        "UPDATE joppa.board_cache SET state = jsonb_set(state, '{decision_required_migrated}', 'false'::jsonb) WHERE workspace=$1",
        &[&ws],
    )
    .await?;
    conn.execute("SELECT pg_stat_statements_reset()", &[])
        .await?;
    store::migrate_notes(&pool).await?;

    let event_count: i64 = conn
        .query_one(
            "SELECT count(*) FROM joppa.events WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0);
    ensure!(
        event_count >= 3_000,
        "expected >= 3000 events, got {event_count}"
    );
    let snap_3k = db_snapshot(&pool, &ws).await?;

    let max_pos_calls: i64 = conn
        .query_one(
            "SELECT COALESCE(sum(calls), 0)::bigint FROM pg_stat_statements
             WHERE query LIKE 'SELECT COALESCE(max(position),%) FROM joppa.events%'",
            &[],
        )
        .await?
        .get(0);
    let full_event_calls: i64 = conn
        .query_one(
            "SELECT COALESCE(sum(calls), 0)::bigint FROM pg_stat_statements
             WHERE query LIKE 'SELECT position,actor,recorded_at::text,command::text FROM joppa.events%'",
            &[],
        )
        .await?
        .get(0);
    ensure!(
        max_pos_calls >= 1 && full_event_calls == 0,
        "migrate_notes must call full_state without loading events: max_pos_calls={max_pos_calls}, full_event_calls={full_event_calls}"
    );

    // Measure peak RSS across 3 consecutive backfill_board + check_journal cycles (6 replays of 3 000+ events).
    let rss_c1 = linux_rss_kb();
    for _ in 0..3 {
        mark_board_stale(&pool, &ws).await?;
        store::backfill_board(&pool).await?;
        store::check_journal(&pool).await?;
    }
    let snap_after = db_snapshot(&pool, &ws).await?;
    ensure!(
        snap_3k == snap_after,
        "3k-event snapshot changed across repeated replays"
    );

    let full_3k = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    let history_len = full_3k["history"]
        .as_array()
        .context("missing history")?
        .len() as i64;
    ensure!(
        history_len == event_count && full_3k["position"] == json!(event_count),
        "Scope::full must stream all {event_count} events, got history={history_len}"
    );
    let dumped_state = full_3k["state"].to_string();
    ensure!(
        dumped_state.contains("[redacted]") && !dumped_state.contains("s3cr3t"),
        "redacted secret leaked in Scope::full state"
    );

    if let (Some(c1), Some(after)) = (rss_c1, linux_rss_kb()) {
        let steady_delta_kb = after - c1;
        println!(
            "Replay 3k VmRSS cycle1={c1} kB, after_6_replays={after} kB, steady_delta={steady_delta_kb} kB"
        );
        ensure!(
            steady_delta_kb < 24_576,
            "VmRSS grew by {steady_delta_kb} kB (>= 24 MB) across 6 replays of {event_count} events"
        );
    }
    Ok(())
}
