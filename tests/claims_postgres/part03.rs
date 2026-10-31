// Part 3 of tests/claims_postgres.rs, included there, so the test names stay unchanged.

async fn permanent_fixture(minutes: i64) -> Result<(deadpool_postgres::Pool, String)> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    claim_time::install(&pool).await?;
    let ws = format!("permanent-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Permanent claims"}),
    )
    .await?;
    for actor in ["bot", "other"] {
        store::execute_input(
            &pool,
            &ws,
            "owner",
            actor,
            &json!({"op":"enroll_service","id":actor,"role":"agent"}),
        )
        .await?;
    }
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    store::execute_remote_input(&pool,&ws,"owner","schema",&json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"types":{"job":{"title":"Jobs","role":"task","status":"none","root":true,"parents":["*"]}},"reason":"Use configured work"})).await?;
    store::execute_remote_input(&pool,&ws,"owner","job",&json!({"op":"save_object","kind":"job","id":"one","revision":0,"parent":null,"data":{"title":"Implement the contract","owner":"owner","body":"Implement and verify the behavior","purpose":"implementation"},"reason":"Create work"})).await?;
    let request = format!(
        "test-claim-time-{}-{}",
        minutes * 60_000,
        uuid::Uuid::new_v4().simple()
    );
    store::execute_remote_input(
        &pool,
        &ws,
        "bot",
        &request,
        &json!({"op":"claim_task","id":"job:one","revision":1}),
    )
    .await?;
    Ok((pool, ws))
}

// [ac:heartbeat-return-ac1]
// [ac:claims-persist-replay]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn elapsed_time_keeps_claims_in_current_queues_and_replay() -> Result<()> {
    for minutes in [31, 120, 1440] {
        let (pool, ws) = permanent_fixture(minutes).await?;
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
        ensure!(
            before["process"]["status"] == "in_progress" && before["process"]["assignee"] == "bot",
            "{before}"
        );
        ensure!(
            store::task_queue(&pool, &ws, "bot", "mine", 10, None).await?["total"] == 1,
            "holder stays in mine"
        );
        ensure!(
            store::task_queue(&pool, &ws, "other", "available", 10, None).await?["total"] == 0,
            "claim stays unavailable"
        );
        let command = json!({"op":"claim_task","id":"job:one","revision":2});
        let refused = store::execute_remote_input(&pool, &ws, "other", "refused", &command)
            .await
            .unwrap_err();
        ensure!(
            refused.to_string() == "task is already claimed",
            "{refused}"
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
        ensure!(before == after, "refusal changes no state");
        store::execute_remote_input(&pool,&ws,"bot","done",&json!({"op":"complete_task","commit":"c0ffee0","id":"job:one","revision":2,"evidence":"Verified after elapsed time"})).await?;
        store::check_journal(&pool).await?;
    }
    Ok(())
}

// [ac:heartbeat-return-ac2]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn heartbeat_refusal_preserves_documents_and_position() -> Result<()> {
    let (pool, ws) = permanent_fixture(1440).await?;
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
    let error = store::execute_remote_input(
        &pool,
        &ws,
        "bot",
        "renew",
        &json!({"op":"heartbeat_task","id":"job:one","revision":2}),
    )
    .await
    .unwrap_err();
    ensure!(
        error.to_string() == "heartbeat_task is no longer supported",
        "{error}"
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
    ensure!(before == after, "refusal changes no state");
    Ok(())
}

// [ac:owner-releases-claim-ac1]
// [ac:old-agent-locked-out-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn explicit_owner_release_keeps_work_and_excludes_the_old_holder() -> Result<()> {
    let (pool, ws) = permanent_fixture(1440).await?;
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
    store::execute_remote_input(
        &pool,
        &ws,
        "owner",
        "release",
        &json!({"op":"release_task","id":"job:one","revision":2,"reason":"Worker unavailable"}),
    )
    .await?;
    let released = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some("job:one"),
            ..Default::default()
        },
    )
    .await?;
    ensure!(
        released["document"]["data"] == before["document"]["data"],
        "release keeps the document"
    );
    ensure!(
        store::task_queue(&pool, &ws, "other", "available", 10, None).await?["total"] == 1,
        "explicit release returns work"
    );
    store::execute_remote_input(
        &pool,
        &ws,
        "other",
        "claim",
        &json!({"op":"claim_task","id":"job:one","revision":3}),
    )
    .await?;
    let error = store::execute_remote_input(
        &pool,
        &ws,
        "bot",
        "old-holder",
        &json!({"op":"complete_task","commit":"c0ffee0","id":"job:one","revision":4,"evidence":"Late completion"}),
    )
    .await
    .unwrap_err();
    ensure!(
        error.to_string() == "Task is not claimed unfinished work",
        "{error}"
    );
    store::check_journal(&pool).await?;
    Ok(())
}
