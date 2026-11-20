//! Black-box check of 500-object projection steps and glibc heap trim after rebuilds (ac:n2eei88j).
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

async fn mark_board_stale(pool: &deadpool_postgres::Pool, ws: &str) -> Result<()> {
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

async fn snapshot(conn: &tokio_postgres::Client, ws: &str) -> Result<Vec<String>> {
    let r = conn
        .query_one(
            "SELECT
                COALESCE(md5(string_agg(address || ':' || revision || ':' || document::text, '|' ORDER BY address)), ''),
                COALESCE(md5((SELECT string_agg(address || ':' || revision || ':' || document::text, '|' ORDER BY address, revision) FROM joppa.object_revisions WHERE workspace=$1)), ''),
                COALESCE(md5((SELECT string_agg(source || ':' || target || ':' || relation, '|' ORDER BY source, target, relation) FROM joppa.object_links WHERE workspace=$1)), ''),
                (SELECT count(*)::text FROM joppa.item_ids i JOIN joppa.board_cache b ON b.workspace=i.workspace WHERE i.workspace=$1 AND i.namespace=COALESCE(b.state->>'reference_prefix', i.workspace)),
                COALESCE(md5((SELECT string_agg(address || ':' || target, '|' ORDER BY address) FROM joppa.object_aliases WHERE workspace=$1)), '')
             FROM joppa.objects WHERE workspace=$1",
            &[&ws],
        )
        .await?;
    Ok((0..5).map(|i| r.get(i)).collect())
}

// ac:n2eei88j
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn objects_project_walks_in_500_steps_and_trims_freed_heap() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let conn = pool.get().await?;
    let _ = conn
        .execute(
            "SELECT joppa.purge_workspace(id) FROM joppa.workspaces WHERE id LIKE 'trim500-%'",
            &[],
        )
        .await;
    store::migrate(&pool).await?;

    let ws1 = format!("trim500-w1-{}", uuid::Uuid::new_v4().simple());
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
         ($1, 1, 'owner', 'init', '{\"op\":\"workspace\",\"title\":\"Trim steps\"}'::jsonb),
         ($1, 2, 'owner', 'dom', '{\"op\":\"domain\",\"id\":\"dom1\",\"title\":\"Dom\",\"owner\":\"owner\",\"description\":\"D\"}'::jsonb),
         ($1, 3, 'owner', 'cap', '{\"op\":\"capability\",\"id\":\"cap1\",\"title\":\"Cap\",\"domain\":\"dom1\",\"owner\":\"owner\",\"description\":\"C\"}'::jsonb),
         ($1, 4, 'owner', 'in-1', '{\"op\":\"intake\",\"id\":\"src-1\",\"said\":\"Intake 1\",\"author\":\"owner\"}'::jsonb)",
        &[&ws1],
    )
    .await?;
    store::backfill_board(&pool)
        .await
        .context("init-backfill")?;
    store::migrate_notes(&pool)
        .await
        .context("init-migrate-notes")?;
    conn.execute(
        "DELETE FROM joppa.objects WHERE workspace=$1 AND address='intake:src-1'",
        &[&ws1],
    )
    .await?;

    let tree = store::tree_schema(&pool, &ws1, "owner", None).await?;
    let mut req_type = tree["schema"]["types"]["requirement"].clone();
    req_type["references"] = json!({});
    let mut ac_type = tree["schema"]["types"]["ac"].clone();
    ac_type["references"] = json!({});
    store::execute_remote_input(
        &pool,
        &ws1,
        "owner",
        "schema-refs",
        &json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "put_types": {"requirement": req_type, "ac": ac_type},
            "reason": "Clear required references for test setup"
        }),
    )
    .await?;

    let pad = "p".repeat(800);
    store::execute_remote_input(
        &pool,
        &ws1,
        "owner",
        "req-1-v1",
        &json!({
            "op": "save_object", "kind": "requirement", "id": "req-1", "revision": 0,
            "parent": "capability:cap1",
            "data": {"title": "Req 1", "owner": "owner", "body": format!("Body 1 {pad}")},
            "reason": "Seed req 1"
        }),
    )
    .await?;
    store::execute_remote_input(
        &pool,
        &ws1,
        "owner",
        "ac-a0",
        &json!({
            "op": "save_object", "kind": "ac", "id": "a0", "revision": 0,
            "parent": "requirement:req-1",
            "data": {"title": "AC 0", "owner": "owner", "body": "Criterion"},
            "reason": "Seed ac"
        }),
    )
    .await?;
    store::execute_remote_input(
        &pool,
        &ws1,
        "owner",
        "sec-1-v1",
        &json!({
            "op": "save_object", "kind": "note", "id": "sec-1", "revision": 0,
            "parent": null,
            "data": {"title": "Secret", "owner": "owner", "body": "Leak v1 SECRET_TOKEN_XYZ"},
            "reason": "Seed secret note"
        }),
    )
    .await?;
    store::execute_remote_input(
        &pool,
        &ws1,
        "owner",
        "sec-1-v2",
        &json!({
            "op": "save_object", "kind": "note", "id": "sec-1", "revision": 1,
            "data": {"body": "Leak v2 SECRET_TOKEN_XYZ"},
            "reason": "Update secret note"
        }),
    )
    .await?;

    // Seed req-2..req-1195 (rev 1) + rev 2 with link to ac:a0 on all 1 195 requirements -> 1 201 objects total.
    let pos0: i64 = conn
        .query_one(
            "SELECT max(position) FROM joppa.events WHERE workspace=$1",
            &[&ws1],
        )
        .await?
        .get(0);
    conn.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command)
         SELECT $1, $2 + (g - 1), 'owner', 'r1-' || g,
                jsonb_build_object('op', 'save_object', 'kind', 'requirement', 'id', 'req-' || g, 'revision', 0,
                    'parent', 'capability:cap1',
                    'data', jsonb_build_object('title', 'Req ' || g, 'owner', 'owner', 'body', 'Body v1 ' || g || ' ' || $3::text),
                    'reason', 'Seed v1')
         FROM generate_series(2::bigint, 1195::bigint) g",
        &[&ws1, &pos0, &pad],
    )
    .await?;
    let req1_rev: i32 = conn
        .query_one(
            "SELECT revision FROM joppa.objects WHERE workspace=$1 AND address='requirement:req-1'",
            &[&ws1],
        )
        .await?
        .get(0);
    let pos1 = pos0 + 1194;
    conn.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command)
         SELECT $1, $2 + g, 'owner', 'r2-' || g,
                jsonb_build_object('op', 'save_object', 'kind', 'requirement', 'id', 'req-' || g,
                    'revision', CASE WHEN g = 1 THEN $4::int ELSE 1 END,
                    'parent', 'capability:cap1',
                    'data', jsonb_build_object('title', 'Req ' || g, 'owner', 'owner', 'body', 'Body v2 ' || g || ' ' || $3::text),
                    'relations', jsonb_build_object('related', jsonb_build_object('edges', jsonb_build_array(jsonb_build_object('to', 'ac:a0')))),
                    'reason', 'Seed v2 with link')
         FROM generate_series(1::bigint, 1195::bigint) g",
        &[&ws1, &pos1, &pad, &req1_rev],
    )
    .await?;
    store::backfill_board(&pool)
        .await
        .context("seed-backfill")?;

    // Redact SECRET_TOKEN_XYZ on stored revisions of note:sec-1.
    let redact_cmd: joppa::commands::Command = serde_json::from_value(json!({
        "op": "redact_object",
        "target": "note:sec-1",
        "fragment": "SECRET_TOKEN_XYZ",
        "reason": "Remove leaked token"
    }))?;
    store::execute(&pool, &ws1, "owner", "redact-sec-1", &redact_cmd).await?;

    let obj_total: i64 = conn
        .query_one(
            "SELECT count(*) FROM joppa.objects WHERE workspace=$1",
            &[&ws1],
        )
        .await?
        .get(0);
    ensure!(obj_total == 1201, "expected 1201 objects, got {obj_total}");
    let leaked: i64 = conn
        .query_one(
            "SELECT (SELECT count(*) FROM joppa.objects WHERE workspace=$1 AND document::text LIKE '%SECRET_TOKEN_XYZ%')
                  + (SELECT count(*) FROM joppa.object_revisions WHERE workspace=$1 AND document::text LIKE '%SECRET_TOKEN_XYZ%')",
            &[&ws1],
        )
        .await?
        .get(0);
    ensure!(leaked == 0, "redacted token leaked before replay: {leaked}");
    let base_snap = snapshot(&conn, &ws1).await?;

    // Replay W 3 times: verify 3 steps of <=500 objects, single link delete, masked redaction, and VmRSS delta <= 16 MB.
    conn.execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements", &[])
        .await?;
    let mut rss_r1 = None;
    for cycle in 1..=3 {
        mark_board_stale(&pool, &ws1).await?;
        conn.execute("SELECT pg_stat_statements_reset()", &[])
            .await?;
        store::backfill_board(&pool)
            .await
            .with_context(|| format!("replay-{cycle}"))?;
        let existing_steps = count_calls(
            &conn,
            "SELECT address,jsonb_build_object(%FROM joppa.objects WHERE workspace=$1 AND address=ANY($2)",
        )
        .await?;
        let id_steps = count_calls(
            &conn,
            "SELECT kind,local_id FROM joppa.item_ids WHERE workspace=$1 AND namespace=$2%",
        )
        .await?;
        let link_del =
            count_calls(&conn, "DELETE FROM joppa.object_links WHERE workspace=%").await?;
        let link_ins = count_calls(
            &conn,
            "INSERT INTO joppa.object_links(workspace,source%jsonb_to_recordset%",
        )
        .await?;
        ensure!(
            existing_steps == 3 && id_steps == 3 && link_del == 1 && link_ins == 3,
            "cycle {cycle}: expected 3 read steps, 1 link delete, 3 link parts, got existing={existing_steps}, ids={id_steps}, ldel={link_del}, lins={link_ins}"
        );
        if cycle == 1 {
            rss_r1 = linux_rss_kb();
        }
    }
    let rss_r3 = linux_rss_kb();
    if let (Some(r1), Some(r3)) = (rss_r1, rss_r3) {
        let delta_kb = r3 - r1;
        println!("Trim500 replay1={r1} kB, replay3={r3} kB, delta={delta_kb} kB");
        ensure!(
            delta_kb <= 16_384,
            "VmRSS after 3rd replay grew by {delta_kb} kB (> 16 MB)"
        );
    }
    ensure!(
        snapshot(&conn, &ws1).await? == base_snap,
        "rows changed after 3 replays"
    );

    // Write 3 objects, then allocate and drop a fragmented heap burst and run set_tree_schema.
    for (i, id) in ["req-1", "req-2", "req-3"].iter().enumerate() {
        let rev: i32 = conn
            .query_one(
                "SELECT revision FROM joppa.objects WHERE workspace=$1 AND address=$2",
                &[&ws1, &format!("requirement:{id}")],
            )
            .await?
            .get(0);
        store::execute_remote_input(
            &pool,
            &ws1,
            "owner",
            &format!("edit-{i}"),
            &json!({"op":"save_object","kind":"requirement","id":id,"revision":rev,"data":{"body":"Updated body"},"reason":"Delta"}),
        )
        .await?;
    }
    let burst: Vec<Value> = (0..60_000)
        .map(|i| json!({"idx": i, "payload": "x".repeat(900)}))
        .collect();
    std::hint::black_box(&burst);
    let rss_peak = linux_rss_kb();
    drop(burst);

    let tree2 = store::tree_schema(&pool, &ws1, "owner", None).await?;
    let mut ac2 = tree2["schema"]["types"]["ac"].clone();
    ac2["question"] = json!("Is the criterion proven");
    conn.execute("SELECT pg_stat_statements_reset()", &[])
        .await?;
    store::execute_remote_input(
        &pool,
        &ws1,
        "owner",
        "schema-step-check",
        &json!({
            "op": "set_tree_schema",
            "revision": tree2["schema"]["revision"],
            "put_types": {"ac": ac2},
            "reason": "Verify 500-object steps and malloc_trim on schema update"
        }),
    )
    .await?;
    let schema_steps = count_calls(
        &conn,
        "SELECT address,jsonb_build_object(%FROM joppa.objects WHERE workspace=$1 AND address=ANY($2)",
    )
    .await?;
    let schema_link_del =
        count_calls(&conn, "DELETE FROM joppa.object_links WHERE workspace=%").await?;
    ensure!(
        schema_steps == 3 && schema_link_del == 1,
        "set_tree_schema must walk 1201 objects in 3 steps with 1 link delete, got steps={schema_steps}, ldel={schema_link_del}"
    );
    if let (Some(r1), Some(peak), Some(after)) = (rss_r1, rss_peak, linux_rss_kb()) {
        println!("Trim500 burst_peak={peak} kB, after_schema_trim={after} kB, base={r1} kB");
        ensure!(
            after + 32_768 < peak && after - r1 <= 24_576,
            "malloc_trim did not return freed burst pages: peak={peak} kB, after={after} kB, base={r1} kB"
        );
    }

    let leaked_after: i64 = conn
        .query_one(
            "SELECT count(*) FROM joppa.object_revisions WHERE workspace=$1 AND document::text LIKE '%SECRET_TOKEN_XYZ%'",
            &[&ws1],
        )
        .await?
        .get(0);
    ensure!(
        leaked_after == 0,
        "redacted token leaked after schema update"
    );

    conn.execute("SELECT joppa.purge_workspace($1)", &[&ws1])
        .await?;
    Ok(())
}
