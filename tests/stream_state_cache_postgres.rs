//! Black-box check of streamed cold state load, skipped summaries, 30k-object LRU cache cap, and peak RSS (ac:ys5ekxs3).
#[path = "common/pg_plan.rs"]
mod pg_plan;
use anyhow::{Context, Result, ensure};
use joppa::{probe, store};
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

async fn setup_template_ws(
    pool: &pg_plan::Pool,
    label: &str,
    domain_total: i64,
    full_sections: bool,
) -> Result<String> {
    let ws = pg_plan::workspace(pool, label).await?;
    let write = async |req: &str, cmd: Value| {
        store::execute_remote_input(pool, &ws, "owner", req, &cmd).await
    };
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    write(
        "schema",
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Configure types",
            "types": {
                "domain": {"title": "Domains", "role": "group", "status": "none", "root": true, "parents": []},
                "requirement": {"title": "Requirements", "role": "requirement", "status": "children", "root": true, "parents": []},
                "ac": {"title": "Criteria", "role": "ac", "status": "children", "parents": ["requirement"]},
                "test": {"title": "Tests", "role": "test", "status": "own", "parents": ["ac"], "title_preview_length": 400},
                "note": {"title": "Inbox", "role": "document", "inbox": true, "status": "none", "root": true, "parents": []}
            }
        }),
    )
    .await?;
    write("d0", json!({"op":"save_object","kind":"domain","id":"d0","revision":0,"parent":null,"data":{"title":"D0","owner":"owner","body":"Template domain"},"reason":"Seed"})).await?;
    if full_sections {
        write("r0", json!({"op":"save_object","kind":"requirement","id":"r0","revision":0,"parent":null,"data":{"title":"R0","owner":"owner","body":"Req"},"reason":"Seed"})).await?;
        write("a0", json!({"op":"save_object","kind":"ac","id":"a0","revision":0,"parent":"requirement:r0","data":{"title":"A0","owner":"owner","body":"AC"},"reason":"Seed"})).await?;
        write("t0", json!({"op":"save_object","kind":"test","id":"t0","revision":0,"parent":"ac:a0","data":{"title":"Old test title","owner":"owner","body":"T"},"reason":"Seed"})).await?;
        store::record_results(
            pool,
            &ws,
            "bot",
            &json!({"commit":"1111111","results":[{"test":"Old test title","status":"pass"}]}),
        )
        .await?;
        write("t0-ren", json!({"op":"save_object","kind":"test","id":"t0","revision":1,"data":{"title":"New test title"},"reason":"Rename"})).await?;
        write("a-arch", json!({"op":"save_object","kind":"ac","id":"arch0","revision":0,"parent":"requirement:r0","data":{"title":"Arch","owner":"owner","body":"AC"},"reason":"Seed"})).await?;
        write(
            "do-arch",
            json!({"op":"archive_object","target":"ac:arch0","revision":1,"reason":"Archive"}),
        )
        .await?;
        write("n0", json!({"op":"save_object","kind":"note","id":"n0","revision":0,"parent":null,"data":{"title":"Inbox 0","owner":"owner","body":"Compact body"},"reason":"Seed"})).await?;
        write(
            "c0",
            json!({"op":"note","target":"ac:a0","id":"c0","text":"Note comment text"}),
        )
        .await?;
    }

    let mut conn = pool.get().await?;
    let existing_objs: i64 = conn
        .query_one(
            "SELECT count(*) FROM joppa.state_entries WHERE workspace=$1 AND section='objects'",
            &[&ws],
        )
        .await?
        .get(0);
    let add_domains = domain_total - existing_objs;
    ensure!(add_domains > 0, "domain_total too small");
    let tx = conn.transaction().await?;
    tx.execute(
        "INSERT INTO joppa.state_entries(workspace,section,key,value,kind,compact)
         SELECT $1, 'objects', 'domain:d' || g, jsonb_set(s.value, '{id}', to_jsonb('d' || g::text)), 'domain', s.compact
         FROM joppa.state_entries s, generate_series(1::bigint, $2::bigint) g
         WHERE s.workspace=$1 AND s.section='objects' AND s.key='domain:d0'",
        &[&ws, &add_domains],
    ).await?;
    tx.execute(
        "INSERT INTO joppa.objects(workspace,address,kind,local_id,revision,parent,present,title,owner,description,document_at,inbox_status,document,signal,process,rollup,test_status)
         SELECT $1, 'domain:d' || g, 'domain', 'd' || g, 1, NULL, true, 'D0', 'owner', 'Template domain', o.document_at, o.inbox_status, o.document, o.signal, o.process, o.rollup, o.test_status
         FROM joppa.objects o, generate_series(1::bigint, $2::bigint) g
         WHERE o.workspace=$1 AND o.address='domain:d0'",
        &[&ws, &add_domains],
    ).await?;
    if full_sections {
        tx.execute("INSERT INTO joppa.state_entries(workspace,section,key,value) SELECT $1,'notes','domain:d'||g,s.value FROM joppa.state_entries s, generate_series(1,7999) g WHERE s.workspace=$1 AND s.section='notes' AND s.key='ac:a0'", &[&ws]).await?;
        tx.execute("INSERT INTO joppa.state_entries(workspace,section,key,value) SELECT $1,'requirements','r'||g,jsonb_set(s.value,'{id}',to_jsonb('r'||g::text)) FROM joppa.state_entries s, generate_series(1,399) g WHERE s.workspace=$1 AND s.section='requirements' AND s.key='r0'", &[&ws]).await?;
        tx.execute("INSERT INTO joppa.state_entries(workspace,section,key,value) SELECT $1,'summaries','r'||g,'{\"status\":\"green\"}'::jsonb FROM generate_series(0,399) g ON CONFLICT DO NOTHING", &[&ws]).await?;
        tx.execute("INSERT INTO joppa.state_entries(workspace,section,key,value) VALUES ($1,'document_overrides','domain:d1','true'::jsonb) ON CONFLICT DO NOTHING", &[&ws]).await?;
    }
    // Change xmin of board_cache so any cached setup state is invalidated (cold cache).
    tx.execute(
        "UPDATE joppa.board_cache SET position=position WHERE workspace=$1",
        &[&ws],
    )
    .await?;
    tx.commit().await?;
    Ok(ws)
}

async fn write_ping(pool: &pg_plan::Pool, ws: &str, tag: &str) -> Result<(u64, u64)> {
    let (l0, r0) = (
        probe::read(&probe::STATE_LOADS),
        probe::read(&probe::STATE_REUSES),
    );
    let cmd = json!({"op":"note","target":"domain:d0","id":format!("ping-{tag}"),"text":format!("Ping {tag}")});
    store::execute_remote_input(pool, ws, "owner", &format!("req-{tag}"), &cmd).await?;
    Ok((
        probe::read(&probe::STATE_LOADS) - l0,
        probe::read(&probe::STATE_REUSES) - r0,
    ))
}

// ac:ys5ekxs3
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn cold_state_read_streams_sections_skips_summaries_and_caps_lru_at_30k_objects() -> Result<()>
{
    let pool = pg_plan::db().await?;
    let w1 = setup_template_ws(&pool, "lru-w1", 20_000, true).await?;
    let w2 = setup_template_ws(&pool, "lru-w2", 6_000, false).await?;
    let w3 = setup_template_ws(&pool, "lru-w3", 6_000, false).await?;

    // Drain search queue before measuring so only W2 has a search task later.
    store::search_pending::pass(&pool).await?;

    let conn = pool.get().await?;
    let (objs_w1, summaries_w1): (i64, i64) = {
        let r = conn.query_one(
            "SELECT count(*) FILTER (WHERE section='objects'), count(*) FILTER (WHERE section='summaries') FROM joppa.state_entries WHERE workspace=$1",
            &[&w1],
        ).await?;
        (r.get(0), r.get(1))
    };
    ensure!(
        objs_w1 == 20_000 && summaries_w1 >= 400,
        "w1 objs={objs_w1}, summaries={summaries_w1}"
    );

    conn.execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements", &[])
        .await?;
    conn.execute("SELECT pg_stat_statements_reset()", &[])
        .await?;
    let rss_before = linux_rss_kb();

    // Cold write in W1 (20k), then W2 (6k -> 26k <= 30k), W3 (6k -> 32k > 30k, W1 leaves -> 12k), then W1 again (32k > 30k, W2 leaves -> {W3, W1} = 26k).
    ensure!(
        write_ping(&pool, &w1, "1").await? == (1, 0),
        "W1 first write must be cold"
    );
    ensure!(
        write_ping(&pool, &w2, "1").await? == (1, 0),
        "W2 first write must be cold"
    );
    ensure!(
        write_ping(&pool, &w3, "1").await? == (1, 0),
        "W3 first write must be cold"
    );
    ensure!(
        write_ping(&pool, &w1, "2").await? == (1, 0),
        "W1 second write reloads and evicts W2"
    );

    // Warm writes on W1 and W3 prove both stayed in STATES (and debug same_as_rows proves W1 field-by-field!).
    ensure!(
        write_ping(&pool, &w1, "3").await? == (0, 1),
        "W1 must stay in STATES and pass same_as_rows"
    );
    ensure!(
        write_ping(&pool, &w3, "2").await? == (0, 1),
        "W3 must stay in STATES"
    );

    // Build search rows of W2 alone: W2 left STATES, so state_cache::read must reload W2 from DB.
    conn.execute(
        "DELETE FROM joppa.search_pending WHERE workspace <> $1",
        &[&w2],
    )
    .await?;
    let loads_before_search = probe::read(&probe::STATE_LOADS);
    store::search_pending::pass(&pool).await?;
    ensure!(
        probe::read(&probe::STATE_LOADS) - loads_before_search == 1,
        "search read of evicted W2 must reload rows"
    );

    ensure!(
        write_ping(&pool, &w2, "2").await? == (1, 0),
        "evicted W2 must reload its rows on next write"
    );

    let rss_cycle1 = linux_rss_kb();
    // Run two more full eviction and 20k-object reload cycles to prove freed LRU pages are reused without unbounded growth.
    for round in ["r2", "r3"] {
        ensure!(
            write_ping(&pool, &w1, &format!("w1-{round}")).await? == (1, 0),
            "evicted W1 must reload and evict W3 on {round}"
        );
        ensure!(
            write_ping(&pool, &w2, &format!("w2a-{round}")).await? == (0, 1),
            "W2 must stay warm after W1 evicts W3 on {round}"
        );
        ensure!(
            write_ping(&pool, &w3, &format!("w3-{round}")).await? == (1, 0),
            "evicted W3 must reload and evict W1 on {round}"
        );
        ensure!(
            write_ping(&pool, &w2, &format!("w2b-{round}")).await? == (0, 1),
            "W2 bump restores [W3, W2] LRU order on {round}"
        );
    }

    if let (Some(before), Some(c1), Some(after)) = (rss_before, rss_cycle1, linux_rss_kb()) {
        let steady_delta_kb = after - c1;
        println!(
            "LRU 30k VmRSS before={before} kB, cycle1={c1} kB, after_3_cycles={after} kB, steady_delta={steady_delta_kb} kB"
        );
        ensure!(
            after < 327_680,
            "VmRSS {after} kB exceeded 320 MB across 32,000 objects and 8,000 notes"
        );
        ensure!(
            steady_delta_kb < 32_768,
            "VmRSS grew by {steady_delta_kb} kB (>= 32 MB) across two extra 20k-object LRU reload cycles"
        );
    }

    let non_summary_w1: i64 = conn
        .query_one(
            "SELECT count(*) FROM joppa.state_entries WHERE workspace=$1 AND section<>'summaries'",
            &[&w1],
        )
        .await?
        .get(0);
    conn.execute("SELECT pg_stat_statements_reset()", &[])
        .await?;
    ensure!(
        write_ping(&pool, &w1, "sql-rows").await? == (1, 0),
        "W1 must reload cold to measure exact SQL rows"
    );
    let r = conn
        .query_one(
            "SELECT rows, query FROM pg_stat_statements WHERE query LIKE '%FROM joppa.state_entries s LEFT JOIN compact%' LIMIT 1",
            &[],
        )
        .await
        .context("state_entries query missing in pg_stat_statements")?;
    let (read_rows, state_sql): (i64, String) = (r.get(0), r.get(1));
    ensure!(
        state_sql.contains("AND s.section<>") && read_rows == non_summary_w1,
        "expected {non_summary_w1} non-summary rows (skipping {summaries_w1} summaries), got {read_rows} on {state_sql}"
    );
    Ok(())
}
