// Part 2 of tests/speed_fixes_http.rs: ac:ewp7bgsu (test:qlggp07y and test:1ggppkwu).

// ac:ewp7bgsu
// test:qlggp07y
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn vector_search_uses_hnsw_on_narrow_filters_and_keeps_all_versions_results() -> Result<()> {
    let _serial = SERIAL.lock().await;
    let (pool, _) = db_url().await?;
    let ws = pg_plan::workspace(&pool, "hnsw-narrow").await?;
    pg_plan::many_nodes(&pool, &ws, 1200, 0).await?;
    // Revise d3 so search_revisions holds a distinct historical revision alongside current docs.
    exec(
        &pool,
        &ws,
        "owner",
        json!({"op":"save_object","kind":"domain","id":"d3","revision":1,"parent":null,"data":{"title":"Historical alpha milestone","owner":"owner"},"reason":"Revise"}),
    )
    .await?;
    store::search_pending::pass(&pool).await?;
    pg_plan::vectors(&pool, &ws).await?;
    pg_plan::analyze(&pool).await?;

    // Check default EXPLAIN plan under SET LOCAL enable_seqscan = off (as candidates::nearest sets)
    // with a narrow filter (owner + kind): must use search_documents_embedding_hnsw and zero Seq Scans!
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let vector: String = tx
        .query_one(
            "SELECT embedding::text FROM joppa.search_documents WHERE workspace=$1 AND local_id='d5'",
            &[&ws],
        )
        .await?
        .get(0);
    let narrow = node_filter::NodeFilter {
        kinds: Some(vec!["domain".into()]),
        owner: Some("owner".into()),
        ..Default::default()
    };
    let bound = node_filter::bind(&*tx, &ws, &narrow, false, false).await?;
    let source = include_str!("../../src/search_candidates.rs");
    let keep = pg_plan::literal(source, "::boolean OR EXISTS(SELECT 1 FROM joppa.objects o")?
        .replace("{flag}", "4")
        .replace("{alias}", "d")
        .replace("{}", &bound.clause(5));
    let sql = pg_plan::literal(source, "SELECT workspace,kind,local_id,rev FROM (")?
        .replace("{revisions}", "")
        .replace("{d}", &keep);
    tx.batch_execute(
        "SET LOCAL hnsw.ef_search = 100; SET LOCAL hnsw.iterative_scan = strict_order; SET LOCAL hnsw.max_scan_tuples = 1000000; SET LOCAL enable_seqscan = off; SET LOCAL enable_bitmapscan = off",
    )
    .await?;
    let scope = vec![ws.clone()];
    let narrows = bound.narrows();
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
        vec![&scope, &PROFILE, &vector, &narrows];
    params.extend(bound.params());
    let plan = pg_plan::explain(&*tx, "FORMAT JSON", &sql, &params).await?;
    tx.rollback().await?;
    let plan_text = plan.to_string();
    ensure!(
        plan_text.contains("search_documents_embedding_hnsw")
            && pg_plan::seq_scans(&plan, "search_documents").is_empty()
            && !plan_text.contains("search_revisions"),
        "narrow filter plan did not stay on HNSW: {plan}"
    );

    // Verify all_versions search finds historical revision 1 of d3 as well as current docs.
    let (_fake, endpoint) = world::provider().await?;
    let embedder = Embedder::new("test-only-key".into(), endpoint)?;
    let reply = store::search::find_with(
        &pool,
        "bot",
        &ws,
        "Domain 3",
        &narrow,
        50,
        Some(&embedder),
        store::search::Flags {
            all_versions: true,
            ..Default::default()
        },
    )
    .await?;
    let items = reply["items"].as_array().context("items")?;
    ensure!(
        items.iter().any(|i| i["id"] == "d3" && i["historical"] == true),
        "expected historical revision of d3 in all_versions reply: {reply}"
    );
    Ok(())
}

// ac:ewp7bgsu
// test:1ggppkwu
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL with pgvector"]
async fn indexer_leaves_live_slots_and_breaker_fast_fails_queries_on_provider_outage() -> Result<()>
{
    let _serial = SERIAL.lock().await;
    let (pool, _) = db_url().await?;
    ensure!(
        store::search_indexer::INDEXER_CONCURRENCY == 2
            && joppa::embeddings_limit::CONCURRENCY == 4,
        "indexer must cap at 2 of 4 slots"
    );

    // 40 pending rows in a fresh clean queue -> one indexer pass spawns at most 2 concurrent requests.
    pool.get()
        .await?
        .batch_execute("DELETE FROM joppa.search_pending; UPDATE joppa.search_documents SET embedding_profile = 'Qwen/Qwen3-Embedding-8B:4096:MRL1024:v1', attempts = 5 WHERE embedding IS NULL; UPDATE joppa.search_revisions SET embedding_profile = 'Qwen/Qwen3-Embedding-8B:4096:MRL1024:v1', attempts = 5 WHERE embedding IS NULL")
        .await?;
    let ws = pg_plan::workspace(&pool, "breaker-slots").await?;
    let run = uuid::Uuid::new_v4().simple().to_string();
    for i in 0..40 {
        exec(
            &pool,
            &ws,
            "owner",
            json!({"op":"save_object","kind":"domain","id":format!("s{i}"),"revision":0,"parent":null,"data":{"title":format!("Slot {run} item {i}"),"owner":"owner"},"reason":"Seed"}),
        )
        .await?;
    }
    let (fake, endpoint) = world::provider().await?;
    fake.delay_ms.store(80, Ordering::SeqCst);
    let embedder = Embedder::new("test-only-key".into(), endpoint)?;
    let settled = store::search_indexer::pass(&pool, &embedder).await?;
    ensure!(settled == 32, "one pass of 2x16 must settle 32 rows, got {settled}");
    ensure!(
        fake.max_open.load(Ordering::SeqCst) <= 2,
        "indexer exceeded 2 concurrent slots: {}",
        fake.max_open.load(Ordering::SeqCst)
    );

    // Input fault (400 via "rejectme") must NOT open the breaker.
    fake.delay_ms.store(0, Ordering::SeqCst);
    for _ in 0..3 {
        let _ = embedder.embed(&["rejectme".to_owned()]).await;
    }
    let ok_vec = embedder
        .embed_query("alpha question", Duration::from_secs(2))
        .await?;
    ensure!(ok_vec.is_some(), "400 input faults must not open the breaker");

    // Two 503 failures open the breaker: the third query returns None at once without calling HTTP!
    fake.down.store(true, Ordering::SeqCst);
    fake.delay_ms.store(2_000, Ordering::SeqCst);
    for _ in 0..2 {
        let v = embedder
            .embed_query("alpha question", Duration::from_millis(150))
            .await?;
        ensure!(v.is_none(), "expected timeout failure");
    }
    let calls_before = fake.calls.load(Ordering::SeqCst);
    let t0 = Instant::now();
    let reply = store::search::find(
        &pool,
        "bot",
        &ws,
        "Slot",
        None,
        10,
        Some(&embedder),
    )
    .await?;
    let elapsed = t0.elapsed();
    ensure!(
        fake.calls.load(Ordering::SeqCst) == calls_before && elapsed < Duration::from_millis(200),
        "open breaker must skip HTTP and answer fast, took {elapsed:?}"
    );
    ensure!(
        reply["semantic_status"] == "unavailable"
            && !reply["items"].as_array().unwrap().is_empty(),
        "expected fast word fallback: {reply}"
    );

    // Provider recovers -> background embed() succeeds and closes the breaker immediately.
    fake.down.store(false, Ordering::SeqCst);
    fake.delay_ms.store(0, Ordering::SeqCst);
    embedder.embed(&["alpha recovery".to_owned()]).await?;
    let recovered = embedder
        .embed_query("alpha question", Duration::from_secs(2))
        .await?;
    ensure!(recovered.is_some(), "a successful embed must close the breaker");
    Ok(())
}
