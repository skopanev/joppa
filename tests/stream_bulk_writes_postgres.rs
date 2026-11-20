//! Black-box check of 500-row chunked projection writes, streamed object and item_id reads, tamper refusals, and peak RSS (ac:q4bn3ofu).
#[path = "common/pg_plan.rs"]
mod pg_plan;
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

async fn count_calls(conn: &tokio_postgres::Client, pattern: &str) -> Result<i64> {
    Ok(conn
        .query_one(
            "SELECT COALESCE(sum(calls), 0)::bigint FROM pg_stat_statements WHERE query LIKE $1",
            &[&pattern],
        )
        .await?
        .get(0))
}

async fn table_counts(conn: &tokio_postgres::Client, ws: &str) -> Result<Vec<i64>> {
    let r = conn
        .query_one(
            "SELECT
                (SELECT count(*) FROM joppa.objects WHERE workspace=$1),
                (SELECT count(*) FROM joppa.object_revisions WHERE workspace=$1),
                (SELECT count(*) FROM joppa.object_links WHERE workspace=$1),
                (SELECT count(*) FROM joppa.item_ids i JOIN joppa.board_cache b ON b.workspace=i.workspace WHERE i.workspace=$1 AND i.namespace=COALESCE(b.state->>'reference_prefix', i.workspace)),
                (SELECT count(*) FROM joppa.object_aliases WHERE workspace=$1),
                (SELECT count(*) FROM joppa.state_entries WHERE workspace=$1 AND section<>'summaries'),
                (SELECT count(*) FROM joppa.state_entries WHERE workspace=$1 AND section='summaries'),
                (SELECT count(*) FROM joppa.state_revisions WHERE workspace=$1)",
            &[&ws],
        )
        .await?;
    Ok((0..8).map(|i| r.get(i)).collect())
}

// ac:q4bn3ofu
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn projection_bulk_writes_chunk_by_500_stream_reads_and_bound_rss() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let conn = pool.get().await?;
    let _ = conn
        .execute(
            "SELECT joppa.purge_workspace(id) FROM joppa.workspaces WHERE id LIKE 'bulk-w%'",
            &[],
        )
        .await;
    store::migrate(&pool).await?;
    let ws1 = format!("bulk-w1-{}", uuid::Uuid::new_v4().simple());
    conn.execute("INSERT INTO joppa.workspaces(id) VALUES($1)", &[&ws1])
        .await?;
    for rule in joppa::commands::REPLAY_RULES {
        conn.execute(
            "INSERT INTO joppa.replay_epochs(workspace,rule,first_position) VALUES($1,$2,1)",
            &[&ws1, &rule],
        )
        .await?;
    }
    conn.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES
         ($1, 1, 'owner', 'init', '{\"op\":\"workspace\",\"title\":\"Bulk writes\"}'::jsonb),
         ($1, 2, 'owner', 'dom', '{\"op\":\"domain\",\"id\":\"dom1\",\"title\":\"Dom\",\"owner\":\"owner\",\"description\":\"D\"}'::jsonb),
         ($1, 3, 'owner', 'cap', '{\"op\":\"capability\",\"id\":\"cap1\",\"title\":\"Cap\",\"domain\":\"dom1\",\"owner\":\"owner\",\"description\":\"C\"}'::jsonb)",
        &[&ws1],
    )
    .await?;
    // 1 201 intakes -> migrate_notes turns them into 1 201 note objects + 1 201 aliases.
    conn.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command)
         SELECT $1, 3 + g, 'owner', 'in-' || g,
                jsonb_build_object('op', 'intake', 'id', 'src-' || g, 'said', 'Intake ' || g, 'author', 'owner')
         FROM generate_series(1::bigint, 1201::bigint) g",
        &[&ws1],
    )
    .await?;
    // 1 201 legacy requirements under cap1 -> 1 201 state_revisions + 1 201 summaries + 1 201 requirement objects.
    conn.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command)
         SELECT $1, 1204 + g, 'owner', 'rev-' || g,
                jsonb_build_object('op', 'revise', 'req', 'req-' || g, 'expected_revision', 0,
                    'capability', 'cap1', 'consumers', '[]'::jsonb, 'title', 'Req ' || g, 'body', 'Body ' || g,
                    'owner', 'owner', 'reason', 'Seed', 'acs', '[]'::jsonb, 'coverage_policy', false, 'carry_tasks', false)
         FROM generate_series(1::bigint, 1201::bigint) g",
        &[&ws1],
    )
    .await?;

    store::backfill_board(&pool)
        .await
        .context("step-backfill-1")?;
    store::migrate_notes(&pool)
        .await
        .context("step-migrate-notes")?;

    // Raise max_tests and max_results to 1 000, add ac:a0 + 750 test objects, and add a 2nd revision + related link on all 1 201 requirements.
    let write1 = async |req: &str, cmd: Value| {
        store::execute_remote_input(&pool, &ws1, "owner", req, &cmd).await
    };
    let tree = store::tree_schema(&pool, &ws1, "owner", None).await?;
    let mut test_rules = tree["schema"]["test_results"].clone();
    test_rules["max_tests"] = json!(1000);
    test_rules["max_results"] = json!(1000);
    let mut req_type = tree["schema"]["types"]["requirement"].clone();
    req_type["root"] = json!(true);
    req_type["references"] = json!({});
    let mut ac_type = tree["schema"]["types"]["ac"].clone();
    ac_type["references"] = json!({});
    write1(
        "schema-cap",
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "put_types": {"requirement": req_type, "ac": ac_type},
            "test_results": test_rules,
            "reason": "Allow root requirements and 1000 results"
        }),
    )
    .await
    .context("step-schema-cap")?;
    write1(
        "a0",
        json!({"op":"save_object","kind":"ac","id":"a0","revision":0,"parent":"requirement:req-1","data":{"title":"A0","owner":"owner","body":"AC"},"reason":"Seed"}),
    )
    .await
    .context("step-save-a0")?;

    let pos_now: i64 = conn
        .query_one(
            "SELECT max(position) FROM joppa.events WHERE workspace=$1",
            &[&ws1],
        )
        .await?
        .get(0);
    let req1_rev: i32 = conn
        .query_one(
            "SELECT revision FROM joppa.objects WHERE workspace=$1 AND address='requirement:req-1'",
            &[&ws1],
        )
        .await?
        .get(0);
    let req2_rev: i32 = conn
        .query_one(
            "SELECT revision FROM joppa.objects WHERE workspace=$1 AND address='requirement:req-2'",
            &[&ws1],
        )
        .await?
        .get(0);
    conn.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command)
         SELECT $1, $2 + g, 'owner', 'link-' || g,
                jsonb_build_object('op', 'save_object', 'kind', 'requirement', 'id', 'req-' || g,
                    'revision', CASE WHEN g = 1 THEN $3::int ELSE $4::int END,
                    'parent', 'capability:cap1',
                    'data', jsonb_build_object('title', 'Req ' || g, 'owner', 'owner', 'body', 'Body v2 ' || g),
                    'relations', jsonb_build_object('related', jsonb_build_object('edges', jsonb_build_array(jsonb_build_object('to', 'ac:a0')))),
                    'reason', 'Link and second revision')
         FROM generate_series(1::bigint, 1201::bigint) g",
        &[&ws1, &pos_now, &req1_rev, &req2_rev],
    )
    .await?;
    let pos_after_links = pos_now + 1201;
    conn.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command)
         SELECT $1, $2 + g, 'owner', 'test-' || g,
                jsonb_build_object('op', 'save_object', 'kind', 'test', 'id', 't-' || g, 'revision', 0,
                    'parent', 'ac:a0',
                    'data', jsonb_build_object('title', 'Test ' || g, 'owner', 'owner', 'body', 'T'),
                    'reason', 'Seed test')
         FROM generate_series(1::bigint, 750::bigint) g",
        &[&ws1, &pos_after_links],
    )
    .await?;
    store::backfill_board(&pool)
        .await
        .context("step-backfill-2")?;
    let counts_w1 = table_counts(&conn, &ws1).await?;

    // Copy the complete journal into a fresh workspace ws2 (zero projection rows) to measure exact 500-row parts on full backfill.
    let ws2 = format!("bulk-w2-{}", uuid::Uuid::new_v4().simple());
    conn.execute("INSERT INTO joppa.workspaces(id) VALUES($1)", &[&ws2])
        .await?;
    conn.execute(
        "INSERT INTO joppa.replay_epochs(workspace,rule,first_position)
         SELECT $2, rule, first_position FROM joppa.replay_epochs WHERE workspace=$1",
        &[&ws1, &ws2],
    )
    .await?;
    conn.execute(
        "INSERT INTO joppa.events(workspace,position,actor,recorded_at,request_id,command,alias)
         SELECT $2, position, actor, recorded_at, request_id, command, alias
         FROM joppa.events WHERE workspace=$1 ORDER BY position",
        &[&ws1, &ws2],
    )
    .await?;

    conn.execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements", &[])
        .await?;
    conn.execute("SELECT pg_stat_statements_reset()", &[])
        .await?;
    store::backfill_board(&pool)
        .await
        .context("step-backfill-2")?;

    let counts_w2 = table_counts(&conn, &ws2).await?;
    ensure!(
        counts_w1 == counts_w2,
        "ws2 counts {counts_w2:?} != ws1 {counts_w1:?}"
    );

    let obj_parts = count_calls(
        &conn,
        "INSERT INTO joppa.objects(workspace,address%jsonb_to_recordset%",
    )
    .await?;
    let rev_parts = count_calls(
        &conn,
        "INSERT INTO joppa.object_revisions(workspace,address%jsonb_to_recordset%",
    )
    .await?;
    let link_del = count_calls(&conn, "DELETE FROM joppa.object_links WHERE workspace=%").await?;
    let link_parts = count_calls(
        &conn,
        "INSERT INTO joppa.object_links(workspace,source%jsonb_to_recordset%",
    )
    .await?;
    let id_parts = count_calls(&conn, "SELECT joppa.register_item(%jsonb_to_recordset%").await?;
    let alias_parts = count_calls(
        &conn,
        "INSERT INTO joppa.object_aliases(workspace,address%jsonb_to_recordset%",
    )
    .await?;
    let state_parts = count_calls(&conn, "INSERT INTO joppa.state_entries(workspace,section,key,value,kind,compact)%jsonb_to_recordset%").await?;
    let summary_parts = count_calls(
        &conn,
        "INSERT INTO joppa.state_entries(workspace,section,key,value) SELECT %jsonb_to_recordset%",
    )
    .await?;
    let srev_parts = count_calls(
        &conn,
        "INSERT INTO joppa.state_revisions(workspace,section%jsonb_to_recordset%",
    )
    .await?;
    ensure!(
        obj_parts >= 5
            && rev_parts >= 8
            && link_del == 1
            && link_parts >= 3
            && id_parts == 4
            && alias_parts >= 3
            && state_parts >= 5
            && summary_parts >= 3
            && srev_parts >= 3,
        "expected 500-row parts across all bulk tables: obj={obj_parts}, rev={rev_parts}, ldel={link_del}, lins={link_parts}, id={id_parts}, alias={alias_parts}, state={state_parts}, sum={summary_parts}, srev={srev_parts}"
    );

    // Change 3 objects on ws2 and record 750 test results (2 parts of 500 and 250).
    let rev_w2 = async |addr: &str| -> Result<i32> {
        Ok(conn
            .query_one(
                "SELECT revision FROM joppa.objects WHERE workspace=$1 AND address=$2",
                &[&ws2, &addr],
            )
            .await?
            .get(0))
    };
    for (i, id) in ["req-1", "req-2", "req-3"].iter().enumerate() {
        let r = rev_w2(&format!("requirement:{id}")).await?;
        store::execute_remote_input(
            &pool,
            &ws2,
            "owner",
            &format!("delta-{i}"),
            &json!({"op":"save_object","kind":"requirement","id":id,"revision":r,"data":{"body":"Delta body"},"reason":"Delta"}),
        )
        .await?;
    }
    let results: Vec<Value> = (1..=750)
        .map(|g| json!({"test": format!("test:t-{g}"), "status": "pass"}))
        .collect();
    conn.execute("SELECT pg_stat_statements_reset()", &[])
        .await?;
    let rec = store::record_results(
        &pool,
        &ws2,
        "owner",
        &json!({"commit": format!("{:040x}", 99), "results": results}),
    )
    .await?;
    ensure!(rec["unbound"] == json!([]), "unexpected unbound: {rec}");
    let tr_parts = count_calls(
        &conn,
        "INSERT INTO joppa.test_results(report_id%jsonb_to_recordset%",
    )
    .await?;
    ensure!(
        tr_parts == 2,
        "750 test_results must insert in 2 parts of <=500, got {tr_parts}"
    );

    // Verify stored revision tamper refusal and clean up immediately so ws2 never stays broken.
    let saved_doc: String = conn
        .query_one(
            "SELECT document::text FROM joppa.object_revisions WHERE workspace=$1 AND address='requirement:req-1' AND revision=1",
            &[&ws2],
        )
        .await?
        .get(0);
    conn.execute(
        "DELETE FROM joppa.objects WHERE workspace=$1 AND address='requirement:req-1'",
        &[&ws2],
    )
    .await?;
    conn.execute(
        "UPDATE joppa.object_revisions SET document = jsonb_set(document, '{reason}', '\"tampered\"'::jsonb)
         WHERE workspace=$1 AND address='requirement:req-1' AND revision=1",
        &[&ws2],
    )
    .await?;
    mark_board_stale(&pool, &ws2).await?;
    let rev_err = store::backfill_board(&pool).await;
    conn.execute(
        "UPDATE joppa.object_revisions SET document = $2::text::jsonb
         WHERE workspace=$1 AND address='requirement:req-1' AND revision=1",
        &[&ws2, &saved_doc],
    )
    .await?;
    store::backfill_board(&pool)
        .await
        .context("step-backfill-2")?;
    ensure!(
        format!("{rev_err:?}").contains("stored object revision differs from the journal"),
        "expected revision tamper refusal, got {rev_err:?}"
    );

    // Verify alias tamper refusal and clean up immediately.
    conn.execute(
        "UPDATE joppa.object_aliases SET target='requirement:req-1' WHERE workspace=$1 AND address='intake:src-1'",
        &[&ws2],
    )
    .await?;
    mark_board_stale(&pool, &ws2).await?;
    let alias_err = store::backfill_board(&pool).await;
    conn.execute(
        "DELETE FROM joppa.object_aliases WHERE workspace=$1 AND address='intake:src-1'",
        &[&ws2],
    )
    .await?;
    store::backfill_board(&pool)
        .await
        .context("step-backfill-2")?;
    ensure!(
        format!("{alias_err:?}").contains("object alias differs from the journal"),
        "expected alias tamper refusal, got {alias_err:?}"
    );

    // Measure peak RSS across 3 full replays of ws2 (streaming objects + item_ids and chunking all bulk writes).
    let rss_c1 = linux_rss_kb();
    for _ in 0..3 {
        mark_board_stale(&pool, &ws2).await?;
        store::backfill_board(&pool)
            .await
            .context("step-backfill-2")?;
    }
    if let (Some(c1), Some(after)) = (rss_c1, linux_rss_kb()) {
        let steady_delta_kb = after - c1;
        println!(
            "BulkWrites 1201 VmRSS cycle1={c1} kB, after_3_replays={after} kB, steady_delta={steady_delta_kb} kB"
        );
        ensure!(
            steady_delta_kb < 24_576,
            "VmRSS grew by {steady_delta_kb} kB (>= 24 MB) across 3 full bulk-write replays"
        );
    }
    conn.execute(
        "SELECT joppa.purge_workspace(id) FROM joppa.workspaces WHERE id IN ($1, $2)",
        &[&ws1, &ws2],
    )
    .await?;
    Ok(())
}
