// Part 3 of tests/journal_probe.rs, included there, so the test names stay unchanged.

// [ac:17a50553-b267-4ecf-8f74-190486759f8d]
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn current_prefix_follows_batch_order_and_does_not_leave_the_transaction()
-> anyhow::Result<()> {
    for rename_first in [true, false] {
        let (pool, ws) = fixture().await?;
        pool.resize(1);
        let backend: i32 = pool
            .get()
            .await?
            .query_one("SELECT pg_backend_pid()", &[])
            .await?
            .get(0);
        let legacy_function: String = pool
            .get()
            .await?
            .query_one(
                "SELECT pg_get_functiondef('joppa.register_item(text,text,text)'::regprocedure)",
                &[],
            )
            .await?
            .get(0);
        let prefix = format!("prefix-{}", uuid::Uuid::new_v4().simple());
        let rename = store::BatchOperation {
            reference: None,
            command: json!({"op":"rename_workspace","title":"Renamed product","reference_prefix":prefix,"reason":"Check batch order"}),
        };
        let create = store::BatchOperation {
            reference: None,
            command: json!({"op":"save_object","kind":"domain","id":"ordered","revision":0,"parent":null,"data":{"title":"Ordered domain","owner":"owner"},"reason":"Setup"}),
        };
        let mut operations = if rename_first {
            vec![rename, create]
        } else {
            vec![create, rename]
        };
        let second_prefix = format!("second-{}", uuid::Uuid::new_v4().simple());
        operations.extend([
            store::BatchOperation { reference: None, command: json!({"op":"rename_workspace","title":"Renamed twice","reference_prefix":second_prefix,"reason":"Check the second batch rename"}) },
            store::BatchOperation { reference: None, command: json!({"op":"save_object","kind":"domain","id":"second","revision":0,"parent":null,"data":{"title":"Second domain","owner":"owner"},"reason":"Setup"}) },
        ]);
        let foreign = format!("foreign-{}", uuid::Uuid::new_v4().simple());
        // Administrative provisioning precedes the normal command probe.
        store::execute_input(
            &pool,
            &foreign,
            "owner",
            "workspace",
            &json!({"op":"workspace","title":"Foreign product"}),
        )
        .await?;
        // Before the rename, item IDs use the prefix that the server derived from the title.
        let title_prefix: String = pool.get().await?.query_one("SELECT COALESCE(state->>'reference_prefix',workspace) FROM joppa.board_cache WHERE workspace=$1", &[&ws]).await?.get(0);
        reset(&pool).await?;
        let result =
            store::execute_batch(&pool, &ws, "owner", "ordered-batch", &operations).await?;
        let create_index = usize::from(rename_first);
        let expected_prefix = if rename_first { &prefix } else { &title_prefix };
        assert!(
            result["results"][create_index]["system_ids"]["domain"]["ordered"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{expected_prefix}-D-"))
        );
        assert!(
            result["results"][3]["system_ids"]["domain"]["second"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{second_prefix}-D-"))
        );
        let other = store::execute_input(
            &pool,
            &foreign,
            "owner",
            "create",
            &json!({"op":"save_object","kind":"domain","id":"other","revision":0,"parent":null,"data":{"title":"Other domain","owner":"owner"},"reason":"Setup"}),
        )
        .await?;
        // Item IDs of the foreign workspace use its own prefix, a slug of its title.
        let foreign_prefix: String = pool.get().await?.query_one("SELECT COALESCE(state->>'reference_prefix',workspace) FROM joppa.board_cache WHERE workspace=$1", &[&foreign]).await?.get(0);
        assert!(
            other["system_ids"]["domain"]["other"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{foreign_prefix}-D-"))
        );
        assert_eq!(journal_selects(&pool).await?["calls"], 0);
        reset(&pool).await?;
        assert!(
            store::execute_input(
                &pool,
                &ws,
                "owner",
                "invalid-create",
                &json!({"op":"note","target":"domain:missing","id":"invalid","text":"A comment on a missing object"})
            )
            .await
            .is_err()
        );
        let inserts: i64 = pool.get().await?.query_one("SELECT COALESCE(sum(calls),0)::bigint FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND userid=(SELECT usesysid FROM pg_user WHERE usename=current_user) AND query ILIKE $1", &[&"%INSERT INTO joppa.events%"]).await?.get(0);
        assert_eq!(
            inserts, 1,
            "rollback test must execute the context-setting event INSERT"
        );
        let row = pool
            .get()
            .await?
            .query_one(
                "SELECT pg_backend_pid(),NULLIF(current_setting('joppa.reference_prefix',true),'')",
                &[],
            )
            .await?;
        assert_eq!(row.get::<_, i32>(0), backend);
        assert!(
            row.get::<_, Option<String>>(1).is_none(),
            "prefix leaked after commit or rollback"
        );
        reset(&pool).await?;
        pool.get()
            .await?
            .execute(
                "SELECT joppa.register_item($1,'domain','admin-probe')",
                &[&ws],
            )
            .await?;
        let admin = journal_selects(&pool).await?;
        assert_eq!(
            admin["calls"], 1,
            "probe must detect the administrative legacy function"
        );
        assert_eq!(admin["rows"], 1);
        store::migrate(&pool).await?;
        assert_eq!(pool.get().await?.query_one("SELECT pg_get_functiondef('joppa.register_item(text,text,text)'::regprocedure)", &[]).await?.get::<_,String>(0), legacy_function);
        println!(
            "Current prefix: rename_first={rename_first}, journal_calls=0, same_backend=true, no_context_leak=true, legacy_admin_probe={admin}, legacy_function_preserved=true"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn heartbeat_refusal_reads_current_rows_and_keeps_documents_and_test_evidence()
-> anyhow::Result<()> {
    let (pool, ws) = fixture().await?;
    let current = store::tree_schema(&pool, &ws, "owner", None).await?;
    let mut types = current["schema"]["types"].clone();
    types["job"] =
        json!({"title":"Jobs","role":"task","status":"none","root":true,"parents":["*"]});
    store::execute_remote_input(&pool,&ws,"owner","jobs",&json!({"op":"set_tree_schema","revision":current["schema"]["revision"],"types":types,"reason":"Configured work"})).await?;
    store::execute_remote_input(&pool,&ws,"owner","job",&json!({"op":"save_object","kind":"job","id":"one","revision":0,"parent":null,"data":{"title":"Implement","owner":"owner","contract":"Implement and verify the behavior","purpose":"implementation","body":"Detailed context ".repeat(60)},"reason":"Create work"})).await?;
    store::execute_remote_input(
        &pool,
        &ws,
        "agent",
        "claim",
        &json!({"op":"claim_task","id":"job:one","revision":1}),
    )
    .await?;
    let before = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("job:one"),
            ..Default::default()
        },
    )
    .await?;
    let snapshot = |pool: Pool, ws: String| async move {
        let client = pool.get().await?;
        let rows=client.query("SELECT section,key,value::text FROM joppa.state_entries WHERE workspace=$1 AND section IN ('task_context','ac_tests','requirements') ORDER BY section,key", &[&ws]).await?;
        Ok::<_, anyhow::Error>(
            rows.iter()
                .map(|row| {
                    (
                        row.get::<_, String>(0),
                        row.get::<_, String>(1),
                        row.get::<_, String>(2),
                    )
                })
                .collect::<Vec<_>>(),
        )
    };
    let evidence = snapshot(pool.clone(), ws.clone()).await?;
    reset(&pool).await?;
    let refused = store::execute_remote_input(
        &pool,
        &ws,
        "agent",
        "heartbeat",
        &json!({"op":"heartbeat_task","id":"job:one","revision":2}),
    )
    .await
    .unwrap_err();
    anyhow::ensure!(
        refused.to_string() == "heartbeat_task is no longer supported",
        "{refused}"
    );
    store::task_queue(&pool, &ws, "agent", "mine", 10, None).await?;
    let probe = journal_selects(&pool).await?;
    anyhow::ensure!(
        probe["calls"] == 0,
        "heartbeat and queue must not select the journal: {probe}"
    );
    let after = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("job:one"),
            ..Default::default()
        },
    )
    .await?;
    anyhow::ensure!(
        before["document"] == after["document"],
        "heartbeat keeps the full document"
    );
    anyhow::ensure!(
        evidence == snapshot(pool.clone(), ws.clone()).await?,
        "heartbeat keeps context and evidence"
    );
    Ok(())
}
