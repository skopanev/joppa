// Part 3 of tests/speed_fixes_http.rs: ac:5jzkeg7c (test:feuogfa5 and test:aoihl73u).

// ac:5jzkeg7c
// test:feuogfa5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn history_filters_use_events_indexes_and_preserve_alias_totals() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, _) = db_url().await?;
    let ws = pg_plan::workspace(&pool, "hist-idx").await?;

    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"domain","id":"d_root","revision":0,"parent":null,"data":{"title":"Root","owner":"owner"},"reason":"Seed"})).await?;
    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"consumer","id":"u1","revision":0,"parent":null,"data":{"title":"User","owner":"owner","body":"Consumer"},"reason":"Seed"})).await?;
    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"capability","id":"c_sub","revision":0,"parent":"domain:d_root","data":{"title":"Cap","owner":"owner","body":"Cap"},"reason":"Seed"})).await?;
    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"requirement","id":"r_sub","revision":0,"parent":"capability:c_sub","data":{"title":"Sub","owner":"owner","body":"Rule"},"reason":"Seed"})).await?;
    let aliased = json!({"op":"save_object","kind":"ac","id":"a_cur","revision":0,"parent":"requirement:r_sub","data":{"title":"Cur","owner":"owner","body":"AC","consumer":"consumer:u1"},"reason":"Seed"});
    let id = uuid::Uuid::new_v4().to_string();
    joppa::actor_alias::scope(
        Some("planner1".into()),
        store::execute_remote_input(&pool, &ws, "bot", &id, &aliased),
    )
    .await?;
    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"ac","id":"a_two","revision":0,"parent":"requirement:r_sub","data":{"title":"Two","owner":"owner","body":"AC","consumer":"consumer:u1"},"reason":"Seed"})).await?;

    // Seed 4,000 bulk filler events + historical aliases (same-kind on a_cur, cross-kind on a_two) to verify index plans and totals.
    let conn = pool.get().await?;
    let base: i64 = conn
        .query_one("SELECT max(position) FROM joppa.events WHERE workspace=$1", &[&ws])
        .await?
        .get(0);
    conn.execute(
        "INSERT INTO joppa.events(workspace,position,request_id,actor,alias,command)
         SELECT $1, $2 + g, 'bulk-' || g, 'bot', CASE WHEN g % 50 = 0 THEN 'planner1' END,
                CASE
                    WHEN g = 1 THEN '{\"op\":\"save_object\",\"kind\":\"ac\",\"id\":\"a_old\"}'::jsonb
                    WHEN g = 2 THEN '{\"op\":\"save_object\",\"kind\":\"legacy_ac\",\"id\":\"x1\"}'::jsonb
                    ELSE jsonb_build_object('op','note','target','domain:other_' || g,'id','n' || g,'text','filler')
                END
         FROM generate_series(1::bigint, 4000::bigint) g",
        &[&ws, &base],
    )
    .await?;
    conn.execute(
        "INSERT INTO joppa.object_aliases(workspace,address,target) VALUES
         ($1,'ac:a_old','ac:a_cur'),
         ($1,'legacy_ac:x1','ac:a_two')",
        &[&ws],
    )
    .await?;
    conn.execute("ANALYZE joppa.events, joppa.object_aliases", &[])
        .await?;

    // Verify EXPLAIN plans use each of the 4 new migration 083 indexes without Seq Scan on events.
    for (sql, idx) in [
        (
            "SELECT position FROM joppa.events e WHERE e.workspace=$1 AND e.command->>'op'='save_object' ORDER BY e.position LIMIT 25",
            "events_workspace_op_position",
        ),
        (
            "SELECT position FROM joppa.events e WHERE e.workspace=$1 AND e.alias='planner1' ORDER BY e.position LIMIT 25",
            "events_workspace_alias_position",
        ),
        (
            "SELECT position FROM joppa.events e WHERE e.workspace=$1 AND joppa.event_raw_address(e.command)=ANY(ARRAY['ac:a_cur','ac:a_old']) ORDER BY e.position LIMIT 25",
            "events_workspace_address_position",
        ),
        (
            "SELECT position FROM joppa.events e WHERE e.workspace=$1 AND split_part(joppa.event_raw_address(e.command),':',1)='ac' ORDER BY e.position LIMIT 25",
            "events_workspace_kind_position",
        ),
    ] {
        let mut c = pool.get().await?;
        let tx = c.transaction().await?;
        tx.batch_execute("SET LOCAL enable_seqscan = off").await?;
        let plan = pg_plan::explain(&*tx, "FORMAT JSON", sql, &[&ws]).await?;
        tx.rollback().await?;
        ensure!(
            plan.to_string().contains(idx) && pg_plan::seq_scans(&plan, "events").is_empty(),
            "expected index {idx} on {sql}: {plan}"
        );
    }

    // Verify joppa_history totals across op, by_alias, kind (including cross-kind alias!), target and under.
    let by_target = store::read_history_lean(&pool, &ws, "bot", &json!({"target": "ac:a_cur"})).await?;
    ensure!(by_target["total"] == 2, "target must include a_cur + a_old: {by_target}");
    let by_cross = store::read_history_lean(&pool, &ws, "bot", &json!({"target": "ac:a_two"})).await?;
    ensure!(by_cross["total"] == 2, "target must include a_two + legacy_ac:x1: {by_cross}");
    let by_under = store::read_history_lean(&pool, &ws, "bot", &json!({"under": "domain:d_root"})).await?;
    ensure!(by_under["total"] == 7, "under d_root must include d_root + c_sub + r_sub + 4 ac events: {by_under}");
    let by_kind = store::read_history_lean(&pool, &ws, "bot", &json!({"kind": "ac"})).await?;
    ensure!(by_kind["total"] == 4, "kind ac must include cross-kind alias legacy_ac:x1: {by_kind}");
    let by_alias = store::read_history_lean(&pool, &ws, "bot", &json!({"by_alias": "planner1"})).await?;
    ensure!(by_alias["total"] == 81, "by_alias planner1 expected 1 + 80 = 81: {by_alias}");
    conn.execute("SELECT joppa.purge_workspace($1)", &[&ws]).await?;
    Ok(())
}

// ac:5jzkeg7c
// test:aoihl73u
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn slow_mcp_and_http_requests_log_one_redacted_line_above_500ms() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, url) = db_url().await?;
    let ws = pg_plan::workspace(&pool, "slow-log").await?;
    exec(
        &pool,
        &ws,
        "owner",
        json!({"op":"save_object","kind":"domain","id":"d1","revision":0,"parent":null,"data":{"title":"Slow check","owner":"owner"},"reason":"Seed"}),
    )
    .await?;
    store::search_pending::pass(&pool).await?;

    let (fake, endpoint) = world::provider().await?;
    let log_path = std::env::temp_dir().join(format!("joppa-slow-{}.log", uuid::Uuid::new_v4()));
    let (child, root) = serve_with_stderr(&url, &endpoint, &log_path)?;
    let _guard = ChildGuard(child);
    for _ in 0..200 {
        if reqwest::get(format!("{root}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Make provider take 560 ms (>= 500 ms) so both MCP joppa_search and HTTP /search log once.
    fake.delay_ms.store(560, Ordering::SeqCst);
    let secret = "top_secret_search_phrase_9911";
    let _ = world::mcp(&root, "bot", "joppa_search", json!({"workspace": &ws, "query": secret})).await?;
    let _ = world::http_search(&root, "bot", &ws, &[("q", secret)]).await?;

    let logs = std::fs::read_to_string(&log_path)?;
    let _ = std::fs::remove_file(&log_path);
    let mcp_lines: Vec<&str> = logs
        .lines()
        .filter(|l| l.starts_with("joppa: slow mcp "))
        .collect();
    let http_lines: Vec<&str> = logs
        .lines()
        .filter(|l| l.starts_with("joppa: slow http "))
        .collect();
    ensure!(
        mcp_lines.len() == 1
            && mcp_lines[0].contains("method=tools/call tool=joppa_search")
            && mcp_lines[0].contains(&format!("workspace={ws}"))
            && mcp_lines[0].contains("actor=bot status=ok ms="),
        "unexpected slow mcp lines: {logs}"
    );
    ensure!(
        http_lines.len() == 1
            && http_lines[0].contains(&format!("method=GET path=/api/workspaces/{ws}/search status=200 ms=")),
        "unexpected slow http lines: {logs}"
    );
    ensure!(
        !logs.contains(secret) && !logs.contains(&world::token("bot")),
        "slow log leaked query text or bearer token: {logs}"
    );
    Ok(())
}
