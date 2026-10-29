// Part 2 of tests/planning_claims_postgres.rs, included there, so the test names stay unchanged.

// planning-claim-ac1 through planning-claim-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn planning_race_publication_release_and_replay_use_the_store() -> Result<()> {
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
    let workspace = planning_workspace(
        &pool,
        &["planner", "other", "worker", "next-worker", "t0-worker"],
    )
    .await?;
    save_work(&pool, &workspace, "planner", "old", 0).await?;
    save_work(&pool, &workspace, "planner", "t0", 0).await?;
    let t0 = read(&pool, &workspace, "work:t0").await?;
    write(
        &pool,
        &workspace,
        "t0-worker",
        json!({"op":"claim_task","id":"work:t0","revision":t0["current_revision"]}),
    )
    .await?;
    let t0_before = read(&pool, &workspace, "work:t0").await?["process"].clone();
    assert_eq!(t0_before["assignee"], "t0-worker");
    let ac = read(&pool, &workspace, "criterion:a").await?;
    assert!(ac["process"]["planning_claim"].is_null());
    let command =
        json!({"op":"claim_planning","ac":"criterion:a","revision":ac["current_revision"]});
    let (left, right) = tokio::join!(
        write(&pool, &workspace, "planner", command.clone()),
        write(&pool, &workspace, "other", command)
    );
    let (holder, claimed) = match (left, right) {
        (Ok(claimed), Err(error)) => {
            assert_eq!(
                error.to_string(),
                "criterion is already claimed for planning"
            );
            ("planner", claimed)
        }
        (Err(error), Ok(claimed)) => {
            assert_eq!(
                error.to_string(),
                "criterion is already claimed for planning"
            );
            ("other", claimed)
        }
        (a, b) => anyhow::bail!("expected one planning holder, got {a:?} and {b:?}"),
    };
    // The claim records the winner and its journal event; existing work keeps its state.
    let claim = read(&pool, &workspace, "criterion:a").await?["process"]["planning_claim"].clone();
    assert_eq!(
        claim,
        json!({"actor":holder,"position":claimed["position"]})
    );
    let events = pool
        .get()
        .await?
        .query(
            "SELECT position,actor FROM joppa.events WHERE workspace=$1 AND command->>'op'='claim_planning' AND command->>'ac'='criterion:a'",
            &[&workspace],
        )
        .await?;
    assert_eq!(events.len(), 1);
    assert_eq!(json!(events[0].get::<_, i64>(0)), claim["position"]);
    assert_eq!(events[0].get::<_, String>(1), holder);
    let t0_after = read(&pool, &workspace, "work:t0").await?["process"].clone();
    assert_eq!(t0_after["status"], t0_before["status"]);
    assert_eq!(t0_after["assignee"], t0_before["assignee"]);
    // The holder of A here may claim a ready criterion in another workspace.
    let second = planning_workspace(&pool, &["planner", "other"]).await?;
    let b = read(&pool, &second, "criterion:b").await?;
    write(
        &pool,
        &second,
        holder,
        json!({"op":"claim_planning","ac":"criterion:b","revision":b["current_revision"]}),
    )
    .await?;
    assert_eq!(
        read(&pool, &second, "criterion:b").await?["process"]["planning_claim"]["actor"],
        holder
    );
    assert_eq!(
        read(&pool, &workspace, "criterion:a").await?["process"]["planning_claim"],
        claim
    );
    save_work(&pool, &workspace, holder, "new", 0).await?;
    let all = store::task_queue(&pool, &workspace, "worker", "all", 100, None).await?;
    let new = all["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["address"] == "work:new")
        .unwrap();
    assert_eq!(new["status"], "open");
    assert_eq!(new["assignee"], Value::Null);
    assert_eq!(new["claimable"], false);
    assert_eq!(
        new["waiting_reason"],
        "Task is awaiting planning publication"
    );
    let direct = write(
        &pool,
        &workspace,
        "worker",
        json!({"op":"claim_task","id":"work:new","revision":1}),
    )
    .await
    .unwrap_err();
    assert_eq!(direct.to_string(), "Task is awaiting planning publication");
    let next = write(&pool, &workspace, "worker", json!({"op":"claim_next_task"})).await?;
    assert_eq!(next["command"]["id"], "work:old");
    let empty = write(
        &pool,
        &workspace,
        "next-worker",
        json!({"op":"claim_next_task"}),
    )
    .await
    .unwrap_err();
    assert_eq!(empty.to_string(), "no available Task matches this claim");

    // A held claim and its publication gate survive a complete projection rebuild.
    let held = read(&pool, &workspace, "criterion:a").await?;
    pool.get()
        .await?
        .execute(
            "DELETE FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    store::migrate(&pool).await?;
    assert_eq!(
        read(&pool, &workspace, "criterion:a").await?["process"],
        held["process"]
    );
    let available =
        store::task_queue(&pool, &workspace, "next-worker", "available", 100, None).await?;
    assert!(available["tasks"].as_array().unwrap().is_empty());
    write(
        &pool,
        &workspace,
        holder,
        json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"}),
    )
    .await?;
    assert!(read(&pool, &workspace, "criterion:a").await?["process"]["planning_claim"].is_null());
    let available =
        store::task_queue(&pool, &workspace, "next-worker", "available", 100, None).await?;
    assert_eq!(available["tasks"][0]["address"], "work:new");
    assert_eq!(
        read(&pool, &workspace, "work:old").await?["process"]["assignee"],
        "worker"
    );

    let ac = read(&pool, &workspace, "criterion:a").await?;
    write(
        &pool,
        &workspace,
        holder,
        json!({"op":"claim_planning","ac":"criterion:a","revision":ac["current_revision"]}),
    )
    .await?;
    let old = read(&pool, &workspace, "work:old").await?;
    save_work(
        &pool,
        &workspace,
        holder,
        "old",
        old["current_revision"].as_u64().unwrap() as u32,
    )
    .await?;
    save_work(&pool, &workspace, holder, "withdrawn", 0).await?;
    let preserved = read(&pool, &workspace, "work:old").await?;
    write(
        &pool,
        &workspace,
        "owner",
        json!({"op":"release_planning","ac":"criterion:a","reason":"Plan withdrawn"}),
    )
    .await?;
    let old = read(&pool, &workspace, "work:old").await?;
    assert_eq!(old["document"], preserved["document"]);
    assert_eq!(old["process"]["assignee"], "worker");
    let retired = read(&pool, &workspace, "work:withdrawn").await?;
    assert_eq!(retired["process"]["status"], "retired");
    assert_eq!(retired["process"]["retired"], "Plan withdrawn");
    assert_eq!(retired["document"]["present"], true);

    // A worker revision must not erase the planner's update-only finish evidence.
    let ac = read(&pool, &workspace, "criterion:a").await?;
    write(
        &pool,
        &workspace,
        holder,
        json!({"op":"claim_planning","ac":"criterion:a","revision":ac["current_revision"]}),
    )
    .await?;
    let old = read(&pool, &workspace, "work:old").await?;
    save_work(
        &pool,
        &workspace,
        holder,
        "old",
        old["current_revision"].as_u64().unwrap() as u32,
    )
    .await?;
    let old = read(&pool, &workspace, "work:old").await?;
    write(&pool, &workspace, "worker", json!({"op":"release_task","id":"work:old","revision":old["current_revision"],"reason":"Return existing work"})).await?;
    write(
        &pool,
        &workspace,
        holder,
        json!({"op":"finish_planning","ac":"criterion:a","reason":"Update complete"}),
    )
    .await?;
    store::check_journal(&pool).await?;
    pool.get()
        .await?
        .execute(
            "DELETE FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    store::migrate(&pool).await?;
    assert_eq!(
        read(&pool, &workspace, "work:withdrawn").await?["process"]["status"],
        "retired"
    );
    assert!(read(&pool, &workspace, "criterion:a").await?["process"]["planning_claim"].is_null());
    store::check_journal(&pool).await?;
    Ok(())
}
