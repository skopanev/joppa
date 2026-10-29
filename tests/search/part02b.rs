// Part of tests/search/part02.rs: the tail of the test before it, included right after it.

/// The hybrid search checks with a test provider, after the full-text reads.
async fn hybrid_search_after_the_index_reads(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    public: &str,
) -> Result<()> {
    let provider = Provider::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let app = Router::new()
        .route("/embeddings", post(embeddings))
        .with_state(provider.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let embedder = Embedder::new("test-only-key".into(), format!("http://{addr}/embeddings"))?;
    let denied =
        store::search::find(pool, "outsider", ws, "renewal", None, 10, Some(&embedder)).await;
    assert!(denied.is_err());
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        0,
        "denied queries must never reach the provider"
    );
    store::search_indexer::drain(pool, &embedder).await?;
    let hybrid = store::search::find(
        pool,
        "agent",
        ws,
        "extend membership",
        Some("requirement"),
        10,
        Some(&embedder),
    )
    .await?;
    assert_eq!(hybrid["mode"], "hybrid");
    assert_eq!(hybrid["semantic_status"], "ready");
    assert_eq!(hybrid["items"][0]["id"], "r");
    assert_eq!(hybrid["embedded"], hybrid["indexed"]);
    provider.fail.store(true, Ordering::SeqCst);
    let fallback = store::search::find(
        pool,
        "agent",
        ws,
        "renewal",
        Some("requirement"),
        10,
        Some(&embedder),
    )
    .await?;
    assert_eq!(fallback["mode"], "full_text");
    assert_eq!(fallback["semantic_status"], "unavailable");
    assert_eq!(fallback["items"][0]["id"], "r");
    provider.fail.store(false, Ordering::SeqCst);
    // Invalidate one document, then edit it again while its old vector is in flight.
    common::save_title(
        pool,
        ws,
        "owner",
        "edit-1",
        "domain:unused",
        "Invoice approval",
        "Clarify",
    )
    .await?;
    provider.pause.store(true, Ordering::SeqCst);
    let p = pool.clone();
    let e = embedder.clone();
    let indexing = tokio::spawn(async move { store::search_indexer::pass(&p, &e).await });
    provider.started.notified().await;
    common::save_title(
        pool,
        ws,
        "owner",
        "edit-2",
        "domain:unused",
        "Archive this scope",
        "Changed again",
    )
    .await?;
    provider.resume.notify_one();
    indexing.await??;
    let row=pool.get().await?.query_one("SELECT embedding IS NULL FROM joppa.search_documents WHERE workspace=$1 AND kind='domain' AND local_id='unused'",&[&ws]).await?;
    assert!(
        row.get::<_, bool>(0),
        "late old vector must not label new wording"
    );
    store::execute_input(
        pool,
        ws,
        "owner",
        "archive",
        &json!({"op":"archive_object","target":"domain:unused","revision":3,"reason":"Out of scope"}),
    )
    .await?;
    assert!(store::search::find(pool,"agent",ws,"archive",Some("domain"),10,None).await?["items"].as_array().unwrap().is_empty());
    // An archived old AC disappears from search.
    let criteria = store::read_objects(
        pool,
        ws,
        "owner",
        &store::ObjectScope {
            parent: Some("requirement:r"),
            ..Default::default()
        },
    )
    .await?;
    for item in criteria["items"].as_array().cloned().unwrap_or_default() {
        store::execute_input(pool, ws, "owner", &format!("remove-{}", item["address"].as_str().unwrap().replace(':', "-")),
            &json!({"op":"archive_object","target":item["address"],"revision":item["revision"],"reason":"Out of scope"})).await?;
    }
    assert!(
        store::search::find(pool, "agent", ws, public, Some("ac"), 10, None).await?["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let new = format!("renamed-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(pool,ws,"owner","rename",&json!({"op":"rename_workspace","workspace_id":new,"reference_prefix":new,"title":"Renamed","reason":"Owner requested"})).await?;
    let found = store::search::find(
        pool,
        "agent",
        ws,
        "renewal",
        Some("requirement"),
        10,
        None,
    )
    .await?;
    assert_eq!(found["workspace"], new);
    assert!(
        found["items"][0]["system_id"]
            .as_str()
            .unwrap()
            .starts_with(&new)
    );
    let after = store::read(pool, ws, "owner", &store::Scope::full()).await?;
    store::migrate(pool).await?;
    assert_eq!(
        store::read(pool, ws, "owner", &store::Scope::full()).await?,
        after
    );
    assert_eq!(
        store::search::find(
            pool,
            "agent",
            &new,
            "renewal",
            Some("requirement"),
            10,
            None
        )
        .await?["items"],
        found["items"]
    );
    // The actual runtime grants support cache refresh without granting journal mutation.
    let role = format!("search_runtime_{}", uuid::Uuid::new_v4().simple());
    pool.get().await?.batch_execute(&format!("CREATE ROLE {role} NOLOGIN; GRANT USAGE ON SCHEMA joppa TO {role}; GRANT SELECT,INSERT ON ALL TABLES IN SCHEMA joppa TO {role}; GRANT USAGE,SELECT ON ALL SEQUENCES IN SCHEMA joppa TO {role}; GRANT UPDATE,DELETE ON joppa.search_documents,joppa.search_state,joppa.search_revisions TO {role}; GRANT UPDATE ON joppa.board_cache,joppa.objects,joppa.state_revisions,joppa.state_entries TO {role}; GRANT DELETE ON joppa.state_entries,joppa.task_queue TO {role}; GRANT DELETE ON joppa.object_links TO {role};")).await?;
    let mut url = reqwest::Url::parse(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    url.query_pairs_mut()
        .append_pair("options", &format!("-c role={role}"));
    let runtime_url = url.as_str().replace("options=-c+role", "options=-c%20role");
    let runtime = store::pool(&runtime_url)?;
    common::save_title(
        &runtime,
        &new,
        "owner",
        "runtime-edit",
        "domain:d",
        "Subscription access",
        "Clarify",
    )
    .await?;
    assert_eq!(
        store::search::find(
            &runtime,
            "agent",
            &new,
            "renewal",
            Some("requirement"),
            10,
            None
        )
        .await?["items"][0]["id"],
        "r"
    );
    let rc = runtime.get().await?;
    assert!(
        rc.execute("DELETE FROM joppa.events WHERE workspace=$1", &[&ws])
            .await
            .is_err()
    );
    assert!(
        rc.execute(
            "UPDATE joppa.command_receipts SET actor='intruder' WHERE workspace=$1",
            &[&ws]
        )
        .await
        .is_err()
    );
    drop(rc);
    runtime.close();
    pool.get()
        .await?
        .batch_execute(&format!("DROP OWNED BY {role}; DROP ROLE {role};"))
        .await?;
    server.abort();
    Ok(())
}
