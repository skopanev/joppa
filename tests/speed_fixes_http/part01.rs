// Part 1 of tests/speed_fixes_http.rs: ac:k5xopwlt (test:kdgt4klp and test:9hytnwvb).

// ac:k5xopwlt
// test:kdgt4klp
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn non_projection_schema_edits_skip_full_rebuild_and_preserve_links_and_compact_rows()
-> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, _) = db_url().await?;
    let ws = pg_plan::workspace(&pool, "fast-schema").await?;

    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    exec(
        &pool,
        &ws,
        "owner",
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Initial schema",
            "types": {
                "requirement": {"title": "Requirements", "role": "requirement", "status": "children", "root": true, "parents": []},
                "ac": {"title": "Criteria", "role": "ac", "status": "children", "parents": ["requirement"]},
                "task": {
                    "title": "Task",
                    "role": "task",
                    "status": "none",
                    "parents": ["ac"],
                    "fields": {"stage": {"type": "enum", "values": ["todo", "done"]}}
                },
                "test": {"title": "Tests", "role": "test", "status": "own", "parents": ["ac"], "title_preview_length": 400},
                "note": {"title": "Inbox", "role": "document", "inbox": true, "status": "none", "root": true, "parents": []}
            }
        }),
    )
    .await?;

    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"requirement","id":"r1","revision":0,"parent":null,"data":{"title":"R1","owner":"owner","body":"Rule"},"reason":"Seed"})).await?;
    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"ac","id":"a1","revision":0,"parent":"requirement:r1","data":{"title":"A1","owner":"owner","body":"AC"},"reason":"Seed"})).await?;
    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"ac","id":"a2","revision":0,"parent":"requirement:r1","data":{"title":"A2","owner":"owner","body":"AC"},"reason":"Seed"})).await?;
    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"test","id":"t1","revision":0,"parent":"ac:a1","data":{"title":"t1","owner":"owner"},"reason":"Seed"})).await?;
    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"task","id":"k1","revision":0,"parent":"ac:a1","data":{"title":"K1","owner":"owner","stage":"todo"},"relations":{"depends_on":{"edges":[{"to":"ac:a2"}]}},"reason":"Seed"})).await?;
    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"note","id":"n1","revision":0,"parent":null,"data":{"title":"Inbox note","owner":"owner","body":"Compact inbox text"},"reason":"Seed"})).await?;
    store::record_results(&pool, &ws, "bot", &json!({"commit":"1111111","results":[{"test":"test:t1","status":"pass"}]})).await?;

    let links_before: i64 = pool
        .get()
        .await?
        .query_one("SELECT count(*) FROM joppa.object_links WHERE workspace=$1", &[&ws])
        .await?
        .get(0);
    ensure!(links_before >= 1, "expected stored link");

    // Non-projection edit via put_types (title, color, badge, field_max_length, done) + priorities:
    // must take DELTA (not REBUILD), read 0 statuses, keep links and not hydrate compact note rows.
    let (reb0, del0, st0) = (
        probe::read(&probe::REBUILD),
        probe::read(&probe::DELTA),
        probe::read(&probe::STATUS),
    );
    let tree2 = store::tree_schema(&pool, &ws, "owner", None).await?;
    let mut updated_types = tree2["schema"]["types"].clone();
    updated_types["task"]["badge"] = json!("TSK");
    updated_types["task"]["color"] = json!("#336699");
    updated_types["task"]["field_max_length"] = json!(8192);
    updated_types["task"]["done"] = json!({"stage": "done"});
    exec(
        &pool,
        &ws,
        "owner",
        json!({
            "op": "set_tree_schema",
            "revision": tree2["schema"]["revision"],
            "reason": "Update non-projection rules",
            "priorities": ["urgent", "normal"],
            "types": updated_types
        }),
    )
    .await?;
    ensure!(probe::read(&probe::REBUILD) == reb0, "non-projection schema edit triggered REBUILD");
    ensure!(probe::read(&probe::DELTA) == del0 + 1, "expected fast DELTA path");
    ensure!(probe::read(&probe::STATUS) == st0, "expected 0 test_results reads on non-projection schema edit");

    let links_after: i64 = pool
        .get()
        .await?
        .query_one("SELECT count(*) FROM joppa.object_links WHERE workspace=$1", &[&ws])
        .await?
        .get(0);
    ensure!(links_after == links_before, "links changed: {links_before} vs {links_after}");
    let board = store::board_read(&pool, &ws, "owner").await?;
    ensure!(board["object_types"]["task"]["badge"] == "TSK", "board look updated: {board}");

    // Projection-changing edit (status rule change on task): MUST trigger REBUILD.
    let reb1 = probe::read(&probe::REBUILD);
    let tree3 = store::tree_schema(&pool, &ws, "owner", None).await?;
    let mut rebuild_types = tree3["schema"]["types"].clone();
    rebuild_types["task"]["status"] = json!("links");
    exec(
        &pool,
        &ws,
        "owner",
        json!({
            "op": "set_tree_schema",
            "revision": tree3["schema"]["revision"],
            "reason": "Change task status rule",
            "types": rebuild_types
        }),
    )
    .await?;
    ensure!(probe::read(&probe::REBUILD) == reb1 + 1, "projection rule change must trigger REBUILD");
    Ok(())
}

// ac:k5xopwlt
// test:9hytnwvb
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn full_rebuild_with_dimensions_reads_test_results_once_and_matches_tones() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, _) = db_url().await?;
    let ws = pg_plan::workspace(&pool, "dim-single-scan").await?;

    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    let mut tr = tree["schema"]["test_results"].clone();
    tr["fields"]
        .as_array_mut()
        .context("fields")?
        .push(json!({"name": "env", "type": "enum", "values": ["dev", "prod"]}));
    exec(
        &pool,
        &ws,
        "owner",
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Configure env dimension",
            "filters": ["env"],
            "test_results": tr,
            "types": {
                "requirement": {
                    "title": "Requirements",
                    "role": "requirement",
                    "status": "children",
                    "root": true,
                    "parents": [],
                    "fields": {"env": {"type": "enum", "values": ["dev", "prod"]}}
                },
                "ac": {"title": "Criteria", "role": "ac", "status": "children", "parents": ["requirement"]},
                "test": {"title": "Tests", "role": "test", "status": "own", "parents": ["ac"], "title_preview_length": 400}
            }
        }),
    )
    .await?;
    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"requirement","id":"r1","revision":0,"parent":null,"data":{"title":"R1","owner":"owner","body":"Rule"},"reason":"Seed"})).await?;
    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"ac","id":"a1","revision":0,"parent":"requirement:r1","data":{"title":"A1","owner":"owner","body":"AC"},"reason":"Seed"})).await?;
    exec(&pool, &ws, "owner", json!({"op":"save_object","kind":"test","id":"t1","revision":0,"parent":"ac:a1","data":{"title":"t1","owner":"owner"},"reason":"Seed"})).await?;
    store::record_results(
        &pool,
        &ws,
        "bot",
        &json!({
            "commit": "2222222",
            "results": [
                {"test": "test:t1", "status": "pass", "env": "dev"},
                {"test": "test:t1", "status": "fail", "env": "prod"}
            ]
        }),
    )
    .await?;

    let conn = pool.get().await?;
    conn.execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements", &[])
        .await?;
    conn.execute("SELECT pg_stat_statements_reset()", &[])
        .await?;

    // Trigger full rebuild on this workspace alone by marking status_projection version 0.
    conn.execute(
        "UPDATE joppa.board_cache SET first_view = jsonb_set(first_view, '{status_projection,version}', '0'::jsonb) WHERE workspace = $1",
        &[&ws],
    )
    .await?;
    store::catch_up_statuses(&pool, false).await?;

    let calls: i64 = conn
        .query_one(
            "SELECT COALESCE(sum(calls), 0)::bigint FROM pg_stat_statements WHERE query LIKE '%FROM joppa.test_results t JOIN joppa.test_reports r%' AND query LIKE '%ORDER BY t.report_id DESC%'",
            &[],
        )
        .await?
        .get(0);
    ensure!(calls == 1, "expected 1 test_results scan on full rebuild with dimensions, got {calls}");

    let row = conn
        .query_one(
            "SELECT dim_tones::text FROM joppa.objects WHERE workspace=$1 AND address='ac:a1'",
            &[&ws],
        )
        .await?;
    let tones: Value = serde_json::from_str(row.get::<_, &str>(0))?;
    ensure!(
        tones["{\"env\":\"dev\"}"] == "green" && tones["{\"env\":\"prod\"}"] == "red",
        "dimension tones mismatch: {tones}"
    );
    Ok(())
}
