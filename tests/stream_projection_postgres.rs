//! Black-box check of streamed full status rebuild, 500-row column parts, single clear, delta equivalence, and peak RSS (ac:pl2gxmtj).
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

type Snapshot = BTreeMap<
    String,
    (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ),
>;

async fn snapshot_columns(pool: &pg_plan::Pool, ws: &str) -> Result<Snapshot> {
    let rows = pool
        .get()
        .await?
        .query(
            "SELECT address, rollup::text, test_status::text, computed::text, dims::text, dim_tones::text
             FROM joppa.objects WHERE workspace=$1 ORDER BY address",
            &[&ws],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get(0), (r.get(1), r.get(2), r.get(3), r.get(4), r.get(5))))
        .collect())
}

async fn mark_stale(pool: &pg_plan::Pool, ws: &str) -> Result<()> {
    pool.get()
        .await?
        .execute(
            "UPDATE joppa.board_cache SET first_view = jsonb_set(first_view, '{status_projection,position}', '0'::jsonb) WHERE workspace=$1",
            &[&ws],
        )
        .await?;
    Ok(())
}

// ac:pl2gxmtj
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn status_rebuild_streams_reads_and_chunks_column_writes_by_500() -> Result<()> {
    let pool = pg_plan::db().await?;
    let ws = pg_plan::workspace(&pool, "stream-proj").await?;
    let write = async |req: &str, cmd: Value| {
        store::execute_remote_input(&pool, &ws, "owner", req, &cmd).await
    };
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    write(
        "schema",
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Configure status, dimensions, and computed fields",
            "filters": ["env"],
            "test_results": {
                "statuses": {"pass": "pass", "fail": "fail", "skip": "skip"},
                "green_if_all": ["pass"],
                "red_if_any": ["fail"],
                "same_commit": "fail_wins",
                "reset_on_change": true,
                "max_tests": 200,
                "max_results": 1000,
                "failure_field": "note",
                "fields": [
                    {"name": "note", "title": "Failure note", "type": "text", "max_length": 1024},
                    {"name": "env", "title": "Environment", "type": "enum", "values": ["dev", "prod"]}
                ]
            },
            "types": {
                "goal": {
                    "title": "Goals",
                    "role": "group",
                    "status": "links",
                    "root": true,
                    "parents": [],
                    "fields": {"stage": {"type": "enum", "values": ["todo", "done"], "from": "links"}}
                },
                "requirement": {
                    "title": "Requirements",
                    "role": "requirement",
                    "status": "children",
                    "root": true,
                    "parents": [],
                    "fields": {
                        "stage": {"type": "enum", "values": ["todo", "done"], "from": "children"},
                        "req_tag": {"type": "enum", "values": ["v1", "v2"], "from": "own"}
                    }
                },
                "ac": {
                    "title": "Criteria",
                    "role": "ac",
                    "status": "children",
                    "parents": ["requirement"],
                    "fields": {
                        "env": {"type": "list", "values": ["dev", "prod"]},
                        "stage": {"type": "enum", "values": ["todo", "done"], "from": "own"},
                        "req_tag": {"type": "enum", "values": ["v1", "v2"], "from": "parent"}
                    }
                },
                "test": {
                    "title": "Tests",
                    "role": "test",
                    "status": "own",
                    "parents": ["ac"],
                    "title_preview_length": 400
                }
            }
        }),
    )
    .await?;

    write("g1", json!({"op":"save_object","kind":"goal","id":"g1","revision":0,"parent":null,"data":{"title":"G1","owner":"owner","body":"Goal"},"reason":"Seed"})).await?;
    write("g-clear", json!({"op":"save_object","kind":"goal","id":"gclear","revision":0,"parent":null,"data":{"title":"GClear","owner":"owner","body":"Unlinked goal"},"reason":"Seed"})).await?;
    write("r1", json!({"op":"save_object","kind":"requirement","id":"r1","revision":0,"parent":null,"data":{"title":"R1","owner":"owner","body":"Req","req_tag":"v1"},"reason":"Seed"})).await?;
    write(
        "a0",
        json!({
            "op":"save_object","kind":"ac","id":"a0","revision":0,"parent":"requirement:r1",
            "data":{"title":"A0","owner":"owner","body":"AC","env":["dev","prod"],"stage":"done"},
            "relations":{"related":{"edges":[{"to":"goal:g1"}]}},
            "reason":"Seed"
        }),
    )
    .await?;
    write("t0", json!({"op":"save_object","kind":"test","id":"t0","revision":0,"parent":"ac:a0","data":{"title":"T0","owner":"owner","body":"Test"},"reason":"Seed"})).await?;
    write("a-arch", json!({"op":"save_object","kind":"ac","id":"arch0","revision":0,"parent":"requirement:r1","data":{"title":"Arch","owner":"owner","body":"AC","stage":"todo"},"reason":"Seed"})).await?;
    write(
        "do-arch",
        json!({"op":"archive_object","target":"ac:arch0","revision":1,"reason":"Archive"}),
    )
    .await?;

    store::record_results(
        &pool,
        &ws,
        "owner",
        &json!({"commit":format!("{:040x}", 1),"results":[
            {"test":"test:t0","status":"pass","env":"dev"},
            {"test":"test:t0","status":"fail","env":"prod","note":"failed on prod"}
        ]}),
    )
    .await?;

    // Catch up any prior workspaces in the shared test DB first so only ws is stale below.
    store::catch_up_statuses(&pool, false).await?;

    // Expand to 1 201 objects (6 seeded + 1 195 cloned criteria across 3 parts of 500 + 500 + 201) plus alias and links.
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute(
        "INSERT INTO joppa.object_aliases(workspace, address, target) VALUES ($1, 'goal:g_old', 'goal:g1') ON CONFLICT DO NOTHING",
        &[&ws],
    )
    .await?;
    tx.execute(
        "INSERT INTO joppa.state_entries(workspace, section, key, value, kind, compact)
         SELECT $1, 'objects', 'ac:a' || g,
                jsonb_set(jsonb_set(s.value, '{id}', to_jsonb('a' || g::text)), '{revisions,0,relations,related,edges,0,to}', '\"goal:g_old\"'::jsonb),
                'ac', s.compact
         FROM joppa.state_entries s, generate_series(1::bigint, 1195::bigint) g
         WHERE s.workspace=$1 AND s.section='objects' AND s.key='ac:a0'",
        &[&ws],
    )
    .await?;
    tx.execute(
        "INSERT INTO joppa.objects(workspace,address,kind,local_id,revision,parent,present,title,owner,description,document_at,inbox_status,document,signal,process,rollup,test_status)
         SELECT $1, 'ac:a' || g, 'ac', 'a' || g, 1, 'requirement:r1', true, 'A0', 'owner', 'AC', o.document_at, o.inbox_status,
                jsonb_set(o.document, '{relations,related,edges,0,to}', '\"goal:g_old\"'::jsonb),
                o.signal, o.process, '{}'::jsonb, NULL
         FROM joppa.objects o, generate_series(1::bigint, 1195::bigint) g
         WHERE o.workspace=$1 AND o.address='ac:a0'",
        &[&ws],
    )
    .await?;
    tx.execute(
        "INSERT INTO joppa.object_links(workspace, source, target, relation, revision, when_label)
         SELECT $1, 'ac:a' || g, 'goal:g_old', 'related', NULL, NULL
         FROM generate_series(1::bigint, 1195::bigint) g",
        &[&ws],
    )
    .await?;
    // Plant stale values on test:t0, goal:gclear, and ac:arch0 so store_column must clear them to NULL after the 500-row parts.
    tx.execute(
        "UPDATE joppa.objects SET computed='{\"stale\":1}'::jsonb WHERE workspace=$1 AND address='test:t0'",
        &[&ws],
    )
    .await?;
    tx.execute(
        "UPDATE joppa.objects SET dims='{\"env\":[\"dev\"]}'::jsonb
         WHERE workspace=$1 AND address='goal:gclear'",
        &[&ws],
    )
    .await?;
    tx.execute(
        "UPDATE joppa.objects SET dim_tones='{\"stale\":\"green\"}'::jsonb
         WHERE workspace=$1 AND address='ac:arch0'",
        &[&ws],
    )
    .await?;
    tx.execute(
        "UPDATE joppa.board_cache
         SET state = jsonb_set(state, '{object_aliases}', '{\"goal:g_old\":\"goal:g1\"}'::jsonb),
             first_view = jsonb_set(first_view, '{status_projection,position}', '0'::jsonb)
         WHERE workspace=$1",
        &[&ws],
    )
    .await?;
    tx.commit().await?;

    let total_objs: i64 = conn
        .query_one(
            "SELECT count(*) FROM joppa.objects WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0);
    ensure!(
        total_objs == 1_201,
        "expected 1201 objects, got {total_objs}"
    );

    conn.execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements", &[])
        .await?;
    conn.execute("SELECT pg_stat_statements_reset()", &[])
        .await?;

    // Run the full status rebuild of ws.
    store::catch_up_statuses(&pool, false).await?;

    let rollup_parts: i64 = conn
        .query_one(
            "SELECT COALESCE(sum(calls), 0)::bigint FROM pg_stat_statements
             WHERE query LIKE 'UPDATE joppa.objects o SET rollup=x.value FROM jsonb_to_recordset%'",
            &[],
        )
        .await?
        .get(0);
    let rollup_clears: i64 = conn
        .query_one(
            "SELECT COALESCE(sum(calls), 0)::bigint FROM pg_stat_statements
             WHERE query LIKE 'UPDATE joppa.objects SET rollup=%WHERE workspace=%AND rollup IS NOT NULL%'",
            &[],
        )
        .await?
        .get(0);
    ensure!(
        rollup_parts == 3 && rollup_clears == 1,
        "1201 objects must write rollup in 3 parts of <=500 and 1 clear, got parts={rollup_parts}, clears={rollup_clears}"
    );

    let snap1 = snapshot_columns(&pool, &ws).await?;
    // Earlier parts (a1 in part 1, a600 in part 2) and final part (a1195 in part 3) must all keep non-null dims, dim_tones, computed, and rollup.
    for addr in ["ac:a1", "ac:a600", "ac:a1195"] {
        let (rol, _ts, comp, dims, dtones) = snap1.get(addr).context(addr)?;
        ensure!(
            rol.is_some() && comp.is_some() && dims.is_some() && dtones.is_some(),
            "{addr} lost a column across 500-row parts: rol={rol:?}, comp={comp:?}, dims={dims:?}, dtones={dtones:?}"
        );
    }
    // test:t0 has no computed rule, goal:gclear has no dims, and ac:arch0 has no dim_tones, so the single clear sets them to NULL.
    let t0_comp = &snap1.get("test:t0").context("t0")?.2;
    let g_dims = &snap1.get("goal:gclear").context("gclear")?.3;
    let arch_dtones = &snap1.get("ac:arch0").context("arch0")?.4;
    ensure!(
        t0_comp.is_none() && g_dims.is_none() && arch_dtones.is_none(),
        "stale columns must clear to NULL: comp={t0_comp:?}, dims={g_dims:?}, arch_dtones={arch_dtones:?}"
    );
    // goal:g1 resolves alias goal:g_old from 1 194 links and gets stage done.
    let g1_comp = snap1
        .get("goal:g1")
        .and_then(|c| c.2.as_deref())
        .unwrap_or("");
    ensure!(
        g1_comp.contains("\"done\""),
        "goal:g1 must compute stage from alias links, got {g1_comp}"
    );

    // Now run writes changing 3 objects (ac:a0, requirement:r1 cascading to 1 196 children, goal:gclear) and prove delta equals a fresh full rebuild.
    let rev = async |addr: &str| -> Result<i32> {
        Ok(conn
            .query_one(
                "SELECT revision FROM joppa.objects WHERE workspace=$1 AND address=$2",
                &[&ws, &addr],
            )
            .await?
            .get(0))
    };
    write("upd-a0", json!({"op":"save_object","kind":"ac","id":"a0","revision":rev("ac:a0").await?,"data":{"stage":"todo"},"reason":"Delta 1"})).await?;
    write("upd-r1", json!({"op":"save_object","kind":"requirement","id":"r1","revision":rev("requirement:r1").await?,"data":{"req_tag":"v2"},"reason":"Delta 2"})).await?;
    write("upd-gc", json!({"op":"save_object","kind":"goal","id":"gclear","revision":rev("goal:gclear").await?,"data":{"title":"GClear2"},"reason":"Delta 3"})).await?;
    let after_delta = snapshot_columns(&pool, &ws).await?;

    let rss_c1 = linux_rss_kb();
    for _ in 0..3 {
        mark_stale(&pool, &ws).await?;
        store::catch_up_statuses(&pool, false).await?;
    }
    let after_full = snapshot_columns(&pool, &ws).await?;
    ensure!(
        after_delta == after_full,
        "delta projection on 3 changed objects diverged from full streamed rebuild"
    );

    if let (Some(c1), Some(after)) = (rss_c1, linux_rss_kb()) {
        let steady_delta_kb = after - c1;
        println!(
            "Projection 1201 VmRSS cycle1={c1} kB, after_3_rebuilds={after} kB, steady_delta={steady_delta_kb} kB"
        );
        ensure!(
            steady_delta_kb < 16_384,
            "VmRSS grew by {steady_delta_kb} kB (>= 16 MB) across 3 full rebuilds"
        );
    }
    Ok(())
}
