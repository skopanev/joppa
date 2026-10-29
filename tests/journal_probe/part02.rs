// Part 2 of tests/journal_probe.rs, included there, so the test names stay unchanged.

// [ac:17a50553-b267-4ecf-8f74-190486759f8d]
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn legacy_retry_projection_preserves_commands_and_batch_receipts() -> anyhow::Result<()> {
    let (pool, ws) = fixture().await?;
    let legacy = json!({"op":"domain","id":"legacy","title":"Legacy domain","authority":"owner"});
    let source = json!({"op":"link_source","req":"r","revision":1,"id":"uri-source","kind":"document","uri":"https://example.test/source","version":"1","relation":"origin"});
    let mut position: i64 = pool
        .get()
        .await?
        .query_one(
            "SELECT max(position) FROM joppa.events WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0);
    for (request, actor, command) in [
        ("legacy-domain", "owner", &legacy),
        ("legacy-uri", "agent", &source),
    ] {
        position += 1;
        pool.get().await?.execute("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,$3,$4,$5::text::jsonb)", &[&ws,&position,&actor,&request,&command.to_string()]).await?;
    }
    pool.get().await?.execute("UPDATE joppa.board_cache SET first_view=first_view-'legacy_receipts_version' WHERE workspace=$1", &[&ws]).await?;
    let before = snapshot(&pool, &ws).await?;
    reset(&pool).await?;
    let fallback = store::execute_input(&pool, &ws, "agent", "legacy-uri", &source).await?;
    assert_eq!(fallback["replayed"], true);
    assert_eq!(journal_selects(&pool).await?["calls"], 1);
    assert_eq!(
        snapshot(&pool, &ws).await?,
        before,
        "unmigrated retry must not repeat an effect"
    );

    pool.get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    store::backfill_board(&pool).await?;
    let before = snapshot(&pool, &ws).await?;
    reset(&pool).await?;
    // An identical retry returns the stored receipt, also for a removed operation.
    for (request, actor, command) in [
        ("legacy-domain", "owner", &legacy),
        ("legacy-uri", "agent", &source),
    ] {
        let retry = store::execute_input(&pool, &ws, actor, request, command).await?;
        assert_eq!(retry["replayed"], true);
    }
    assert_eq!(journal_selects(&pool).await?["calls"], 0);
    assert_eq!(snapshot(&pool, &ws).await?, before);
    assert!(
        store::execute_input(&pool, &ws, "agent", "legacy-domain", &legacy)
            .await
            .is_err()
    );
    let mut changed = legacy.clone();
    changed["title"] = json!("Different content");
    assert!(
        store::execute_input(&pool, &ws, "owner", "legacy-domain", &changed)
            .await
            .is_err()
    );
    // A new request with a removed operation is refused.
    let refused = store::execute_input(&pool, &ws, "owner", "new-legacy", &legacy)
        .await
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("domain was removed; use save_object")
    );
    let single = [store::BatchOperation {
        reference: None,
        command: legacy,
    }];
    assert!(
        store::execute_batch(&pool, &ws, "owner", "legacy-domain", &single)
            .await
            .is_err()
    );

    let operations = [store::BatchOperation {
        reference: Some("first".into()),
        command: json!({"op":"save_object","kind":"domain","id":"batch-group","revision":0,"parent":null,"data":{"title":"Batch group","owner":"owner"},"reason":"Setup"}),
    }];
    let batch = store::execute_batch(&pool, &ws, "owner", "outer-batch", &operations).await?;
    let batch_position = batch["results"][0]["position"].as_i64().unwrap();
    let private_request: String = pool
        .get()
        .await?
        .query_one(
            "SELECT request_id FROM joppa.events WHERE workspace=$1 AND position=$2",
            &[&ws, &batch_position],
        )
        .await?
        .get(0);
    reset(&pool).await?;
    let replay = store::execute_batch(&pool, &ws, "owner", "outer-batch", &operations).await?;
    assert_eq!(replay["position"], batch["position"]);
    assert_eq!(replay["replayed"], true);
    let changed = [store::BatchOperation {
        reference: Some("different".into()),
        command: operations[0].command.clone(),
    }];
    assert!(
        store::execute_batch(&pool, &ws, "owner", "outer-batch", &changed)
            .await
            .is_err()
    );
    assert_eq!(journal_selects(&pool).await?["calls"], 0);
    let receipt: String = pool.get().await?.query_one("SELECT input::text FROM joppa.command_receipts WHERE workspace=$1 AND request_id='outer-batch'", &[&ws]).await?.get(0);
    pool.get().await?.execute("UPDATE joppa.board_cache SET first_view=first_view-'legacy_receipts_version' WHERE workspace=$1", &[&ws]).await?;
    store::migrate(&pool).await?;
    store::migrate(&pool).await?;
    assert!(!pool.get().await?.query_one("SELECT EXISTS(SELECT 1 FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2)", &[&ws,&private_request]).await?.get::<_,bool>(0));
    assert!(
        store::execute_input(
            &pool,
            &ws,
            "owner",
            &private_request,
            &operations[0].command
        )
        .await
        .is_err()
    );
    assert_eq!(pool.get().await?.query_one("SELECT input::text FROM joppa.command_receipts WHERE workspace=$1 AND request_id='outer-batch'", &[&ws]).await?.get::<_,String>(0), receipt);
    reset(&pool).await?;
    save_title(
        &pool,
        &ws,
        "owner",
        "after-migration",
        "domain:d",
        "After migration",
        "Check marker persistence",
    )
    .await?;
    save_title(
        &pool,
        &ws,
        "owner",
        "after-projection",
        "domain:d",
        "After projection",
        "Check marker persistence",
    )
    .await?;
    assert_eq!(journal_selects(&pool).await?["calls"], 0);
    let incremental = snapshot(&pool, &ws).await?;
    pool.get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    store::backfill_board(&pool).await?;
    assert_eq!(snapshot(&pool, &ws).await?, incremental);
    assert!(!pool.get().await?.query_one("SELECT EXISTS(SELECT 1 FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2)", &[&ws,&private_request]).await?.get::<_,bool>(0));
    assert!(
        store::execute_input(
            &pool,
            &ws,
            "owner",
            &private_request,
            &operations[0].command
        )
        .await
        .is_err()
    );
    let replay = store::execute_batch(&pool, &ws, "owner", "outer-batch", &operations).await?;
    assert_eq!(replay["results"], batch["results"]);
    assert_eq!(replay["replayed"], true);
    println!(
        "Legacy retries: fallback_detected=true, migrated_journal_calls=0, alias=true, uri=true, actor_and_content_guard=true, complete_batch_receipt=true, marker_persists=true"
    );
    Ok(())
}
