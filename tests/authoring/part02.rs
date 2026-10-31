// Part 2 of tests/authoring.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn claim_next_uses_ordered_priorities_then_falls_back() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "test DB only");
    store::migrate(&pool).await?;
    let ws = format!("priority-{}", uuid::Uuid::new_v4().simple());
    let public_ws = format!("public-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "create",
        &json!({"op":"workspace","title":"Priority"}),
    )
    .await?;
    store::execute_input(&pool, &ws, "owner", "rename",
        &json!({"op":"rename_workspace","workspace_id":public_ws,"title":"Public Priority","reason":"Public ID"})).await?;
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "consumer",
        &json!({"op":"save_object","kind":"consumer","id":"buyer","revision":0,"parent":null,"data":{"title":"Buyer","owner":"owner"},"reason":"Setup"}),
    )
    .await?;
    for (suffix, domain) in [("old", "ordinary"), ("new", "urgent")] {
        store::execute_input(
            &pool,
            &ws,
            "owner",
            &format!("domain-{suffix}"),
            &json!({"op":"save_object","kind":"domain","id":domain,"revision":0,"parent":null,"data":{"title":domain,"owner":"owner"},"reason":"Setup"}),
        )
        .await?;
        store::execute_input(
            &pool,
            &ws,
            "owner",
            &format!("cap-{suffix}"),
            &json!({"op":"save_object","kind":"capability","id":format!("cap-{suffix}"),"revision":0,"parent":format!("domain:{domain}"),"data":{"title":suffix,"owner":"owner"},"reason":"Setup"}),
        )
        .await?;
        store::execute_input(
            &pool,
            &ws,
            "owner",
            &format!("req-{suffix}"),
            &json!({"op":"save_object","kind":"requirement","id":format!("req-{suffix}"),"revision":0,
                "parent":format!("capability:cap-{suffix}"),"data":{"title":suffix,"body":"Scope","owner":"owner"},"reason":"Start"}),
        )
        .await?;
        store::execute_input(&pool, &ws, "owner", &format!("ac-{suffix}"),
            &json!({"op":"save_object","kind":"ac","id":format!("ac-{suffix}"),"revision":0,
                "parent":format!("requirement:req-{suffix}"),"data":{"title":"Works","owner":"owner","consumer":"buyer",
                "before_launch":false,"decision_required":false,"depends_on":[]},"reason":"Start"})).await?;
        store::execute_input(&pool, &ws, "owner", &format!("assess-{suffix}"),
            &json!({"op":"assess","req":format!("requirement:req-{suffix}"),"revision":2,
                "ac":format!("ac:ac-{suffix}"),"assessment":{"ready":true,"reason":"Ready","owner":null,"decision":null}})).await?;
        store::execute_input(&pool, &ws, "owner", &format!("task-{suffix}"),
            &json!({"op":"save_object","kind":"task","id":format!("task-{suffix}"),"revision":0,
                "parent":format!("ac:ac-{suffix}"),"data":{"title":suffix,"owner":"owner","projects":[],
                "purpose":"implementation","body":"Implement and provide evidence"},"reason":"Plan the work"})).await?;
    }
    let original = store::task_queue(&pool, &ws, "owner", "all", 50, None).await?;
    assert_eq!(original["workspace"], public_ws);
    let created_at = original["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == "task:task-old")
        .unwrap()["created_at"]
        .as_u64()
        .unwrap();
    assert!(created_at > 0);
    pool.get().await?.execute(
        "UPDATE joppa.task_queue SET created_at=0,data=jsonb_set(data,'{created_at}','0'::jsonb) WHERE workspace=$1 AND address='task:task-old'",
        &[&ws],
    ).await?;
    assert_eq!(
        store::task_queue(&pool, &public_ws, "owner", "all", 50, None).await?["tasks"][0]["created_at"],
        0
    );
    assert!(store::backfill_board(&pool).await? >= 1);
    let repaired = store::task_queue(&pool, &public_ws, "owner", "all", 50, None).await?;
    assert_eq!(repaired["workspace"], public_ws);
    assert_eq!(
        repaired["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == "task:task-old")
            .unwrap()["created_at"],
        created_at
    );
    let canonical_priorities = vec!["task:task-new".to_owned()];
    let canonical_preview = store::task_queue_filtered(
        &pool,
        &ws,
        "owner",
        "available",
        1,
        None,
        None,
        None,
        &canonical_priorities,
    )
    .await?;
    assert_eq!(canonical_preview["tasks"][0]["id"], "task:task-new");
    let canonical_claim = store::execute_input(
        &pool,
        &ws,
        "owner",
        "canonical-next",
        &json!({"op":"claim_next_task","priorities":canonical_priorities}),
    )
    .await?;
    assert_eq!(canonical_claim["command"]["req"], Value::Null);
    assert_eq!(canonical_claim["command"]["id"], "task:task-new");
    store::execute_input(&pool,&ws,"owner","release-canonical",&json!({"op":"release_task","revision":2,"id":"task:task-new","reason":"Return the canonical priority task"})).await?;
    let mut priorities = vec!["domain:urgent"; 101];
    priorities.push("requirement:req-old");
    let preview = store::task_queue_filtered(
        &pool,
        &ws,
        "owner",
        "available",
        1,
        None,
        None,
        None,
        &priorities.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
    .await?;
    assert_eq!(preview["tasks"][0]["id"], "task:task-new");
    assert_eq!(
        store::task_queue(&pool, &ws, "owner", "available", 50, None).await?["total"],
        2
    );
    let request = json!({"op":"claim_next_task","priorities":priorities});
    let first = store::execute_input(&pool, &ws, "owner", "next-1", &request).await?;
    assert_eq!(first["command"]["id"], "task:task-new");
    let second = store::execute_input(&pool, &ws, "owner", "next-2", &request).await?;
    assert_eq!(second["command"]["id"], "task:task-old");
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "release-old",
        &json!({"op":"release_task","revision":2,"id":"task:task-old","reason":"Return"}),
    )
    .await?;
    let fallback = store::execute_input(
        &pool,
        &ws,
        "owner",
        "next-fallback",
        &json!({"op":"claim_next_task","priorities":["domain:urgent"]}),
    )
    .await?;
    assert_eq!(fallback["command"]["id"], "task:task-old");
    assert!(
        store::execute_input(
            &pool,
            &ws,
            "owner",
            "bad-priority",
            &json!({"op":"claim_next_task","priorities":["domain:typo"]})
        )
        .await
        .is_err()
    );
    Ok(())
}
