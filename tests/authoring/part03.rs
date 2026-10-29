// Part 3 of tests/authoring.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_saved_task_moves_through_the_queue_a_claim_race_and_completion() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "test DB only");
    store::migrate(&pool).await?;
    let ws = format!("carry-{}", uuid::Uuid::new_v4().simple());
    let save = |kind: &str, id: &str, parent: Option<&str>, data: Value| json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"reason":"Setup"});
    for (id, actor, command) in [
        (
            "create",
            "owner",
            json!({"op":"workspace","title":"Task carry"}),
        ),
        (
            "member",
            "owner",
            json!({"op":"enroll_service","id":"agent","role":"agent"}),
        ),
        (
            "consumer",
            "owner",
            save(
                "consumer",
                "buyer",
                None,
                json!({"title":"Buyer","owner":"owner"}),
            ),
        ),
        (
            "domain",
            "agent",
            save(
                "domain",
                "area",
                None,
                json!({"title":"Area","owner":"owner"}),
            ),
        ),
        (
            "capability",
            "agent",
            save(
                "capability",
                "cap",
                Some("domain:area"),
                json!({"title":"Ability","owner":"owner"}),
            ),
        ),
        (
            "requirement",
            "agent",
            save(
                "requirement",
                "req",
                Some("capability:cap"),
                json!({"title":"Requirement","body":"Scope","owner":"owner"}),
            ),
        ),
        (
            "criterion",
            "agent",
            save(
                "ac",
                "ac",
                Some("requirement:req"),
                json!({"title":"Works","owner":"owner","consumer":"buyer","before_launch":false,"decision_required":false,"depends_on":[]}),
            ),
        ),
        (
            "wrong-project",
            "owner",
            json!({"op":"project","id":"wrong","repository":"https://example.test/wrong"}),
        ),
        (
            "right-project",
            "owner",
            json!({"op":"project","id":"mobileapps","repository":"https://example.test/mobileapps"}),
        ),
    ] {
        store::execute_input(&pool, &ws, actor, id, &command).await?;
    }
    let task = "task:open-task";
    let revision = || async {
        anyhow::Ok(
            store::read_objects(
                &pool,
                &ws,
                "owner",
                &store::ObjectScope {
                    address: Some(task),
                    ..Default::default()
                },
            )
            .await?["current_revision"]
                .clone(),
        )
    };
    let data = |contract: &str, project: &str| json!({"title":"Build access flow","owner":"owner","projects":[project],"purpose":"implementation","contract":contract});
    store::execute_input(
        &pool,
        &ws,
        "agent",
        "open-task",
        &save(
            "task",
            "open-task",
            Some("ac:ac"),
            data(
                "Implement the revised access flow and attach merged build evidence",
                "wrong",
            ),
        ),
    )
    .await?;
    // The creator corrects the project before a claim.
    let mut fix = save(
        "task",
        "open-task",
        Some("ac:ac"),
        data(
            "Implement the revised mobile access flow and attach merged build evidence",
            "mobileapps",
        ),
    );
    fix["revision"] = revision().await?;
    store::execute_input(&pool, &ws, "agent", "amend-project", &fix).await?;
    assert_eq!(
        store::task_queue(&pool, &ws, "agent", "waiting", 50, None).await?["total"],
        1
    );
    assert_eq!(
        store::task_queue(&pool, &ws, "agent", "available", 50, None).await?["total"],
        0
    );
    store::execute_input(&pool,&ws,"agent","ready",&json!({"op":"assess","req":"requirement:req","revision":2,"ac":"ac:ac","assessment":{"ready":true,"reason":"Contract is clear","owner":null,"decision":null}})).await?;
    let queue = store::task_queue(&pool, &ws, "agent", "available", 1, None).await?;
    assert_eq!(queue["tasks"][0]["id"], task);
    assert_eq!(queue["tasks"][0]["projects"], json!(["mobileapps"]));
    let page = store::task_queue(&pool, &ws, "agent", "all", 1, None).await?;
    assert!(
        store::task_queue(&pool, &ws, "outsider", "all", 1, None)
            .await
            .is_err()
    );
    let next = json!({"op":"claim_next_task"});
    let (owner_result, agent_result) = tokio::join!(
        store::execute_input(&pool, &ws, "owner", "claim-owner", &next),
        store::execute_input(&pool, &ws, "agent", "claim-agent", &next)
    );
    assert_ne!(owner_result.is_ok(), agent_result.is_ok());
    let (winner, winner_request, loser, success) = if let Ok(result) = owner_result {
        ("owner", "claim-owner", "agent", result)
    } else {
        ("agent", "claim-agent", "owner", agent_result?)
    };
    assert_eq!(success["command"]["op"], "claim_task");
    assert_eq!(success["command"]["id"], task);
    assert!(page["next_cursor"].is_null(), "{page}");
    assert_eq!(
        store::execute_input(&pool, &ws, winner, winner_request, &next).await?["replayed"],
        true
    );
    assert_eq!(
        store::task_queue(&pool, &ws, loser, "available", 50, None).await?["total"],
        0
    );
    assert_eq!(
        store::task_queue(&pool, &ws, winner, "mine", 50, None).await?["tasks"][0]["status"],
        "in_progress"
    );
    store::execute_input(
        &pool,
        &ws,
        winner,
        "release",
        &json!({"op":"release_task","revision":revision().await?,"id":task,"reason":"Handoff"}),
    )
    .await?;
    store::execute_input(
        &pool,
        &ws,
        loser,
        "claim-explicit",
        &json!({"op":"claim_task","revision":revision().await?,"id":task}),
    )
    .await?;
    store::execute_input(&pool,&ws,loser,"complete",&json!({"op":"complete_task","commit":"c0ffee0","revision":revision().await?,"id":task,"evidence":"merged build abc"})).await?;
    assert_eq!(
        store::task_queue(&pool, &ws, loser, "mine", 50, None).await?["total"],
        0
    );
    assert_eq!(
        store::task_queue(&pool, &ws, loser, "all", 50, None).await?["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == task)
            .unwrap()["status"],
        "done"
    );
    // The journal rebuilds the same state.
    let after = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    store::migrate(&pool).await?;
    assert_eq!(
        store::read(&pool, &ws, "owner", &store::Scope::full()).await?,
        after
    );
    Ok(())
}
