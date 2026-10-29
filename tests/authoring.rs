mod common;
#[path = "common/resolve_id.rs"]
mod resolve_id;
use anyhow::Result;
use joppa::store;
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn generated_ids_and_atomic_batches_survive_retries_failures_and_concurrency() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(db.starts_with("joppa_test"), "test DB only");
    store::migrate(&pool).await?;
    let ws = format!("authoring-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "create",
        &json!({"op":"workspace","title":"Example"}),
    )
    .await?;
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "member",
        &json!({"op":"enroll_service","id":"agent","role":"agent"}),
    )
    .await?;
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "consumer",
        &json!({"op":"save_object","kind":"consumer","id":"buyer","revision":0,"parent":null,"data":{"title":"Buyer","owner":"owner"},"reason":"Setup"}),
    )
    .await?;
    let input = json!({"op":"save_object","kind":"domain","revision":0,"parent":null,"data":{"title":"Area","owner":"owner"},"reason":"Create"});
    let (a, b) = tokio::join!(
        store::execute_input(&pool, &ws, "agent", "auto", &input),
        store::execute_input(&pool, &ws, "agent", "auto", &input)
    );
    let a = a?;
    let b = b?;
    assert_eq!(a["command"], b["command"]);
    assert_eq!(a["system_ids"], b["system_ids"]);
    assert_ne!(a["replayed"], b["replayed"]);
    let id = a["command"]["id"].as_str().unwrap();
    uuid::Uuid::parse_str(id)?;
    let public_id = a["system_ids"]["domain"][id].as_str().unwrap();
    // Item IDs start with the prefix of the workspace, a slug of its title.
    let prefix: String = pool.get().await?.query_one("SELECT COALESCE(state->>'reference_prefix',workspace) FROM joppa.board_cache WHERE workspace=$1", &[&ws]).await?.get(0);
    assert!(public_id.starts_with(&format!("{prefix}-D-")));
    assert_eq!(
        resolve_id::resolve(&pool, "owner", &ws, public_id).await?,
        format!("domain:{id}")
    );
    assert!(
        store::execute_input(&pool, &ws, "owner", "auto", &input)
            .await
            .is_err()
    );
    assert!(
        store::execute_input(
            &pool,
            &ws,
            "agent",
            "auto",
            &json!({"op":"save_object","kind":"domain","revision":0,"parent":null,"data":{"title":"Changed","owner":"owner"},"reason":"Create"})
        )
        .await
        .is_err()
    );
    assert!(
        store::execute_input(&pool, &ws, "outsider", "auto", &input)
            .await
            .is_err()
    );
    let operations: Vec<store::BatchOperation> = serde_json::from_value(json!([
        {"ref":"area","command":{"op":"save_object","kind":"domain","revision":0,"parent":null,"data":{"title":"Second area","owner":"owner"},"reason":"Agreed scope"}},
        {"ref":"ability","command":{"op":"save_object","kind":"capability","revision":0,"parent":{"$ref":"area/address"},"data":{"title":"Keep paid access","owner":"owner"},"reason":"Agreed scope"}},
        {"ref":"requirement","command":{"op":"save_object","kind":"requirement","revision":0,"parent":{"$ref":"ability/address"},"data":{"title":"Paid access persists","body":"Only iOS","owner":"owner"},"reason":"Agreed scope"}},
        {"ref":"criterion","command":{"op":"save_object","kind":"ac","revision":0,"parent":{"$ref":"requirement/address"},"data":{"title":"On iOS access survives sign-in","owner":"owner","consumer":"buyer","before_launch":false,"decision_required":false,"depends_on":[]},"reason":"Agreed scope"}},
        {"command":{"op":"comment","req":{"$ref":"requirement/address"},"revision":2,"text":"Please review","kind":"comment","ac":{"$ref":"criterion/address"}}}
    ]))?;
    let (a, b) = tokio::join!(
        store::execute_batch(&pool, &ws, "agent", "batch", &operations),
        store::execute_batch(&pool, &ws, "agent", "batch", &operations)
    );
    let a = a?;
    let b = b?;
    assert_eq!(a["results"], b["results"]);
    assert_ne!(a["replayed"], b["replayed"]);
    assert_eq!(
        a["results"][1]["command"]["parent"],
        format!(
            "domain:{}",
            a["results"][0]["command"]["id"].as_str().unwrap()
        )
    );
    let req = a["results"][2]["command"]["id"].as_str().unwrap();
    let ac = a["results"][3]["command"]["id"].as_str().unwrap();
    let domain = a["results"][0]["command"]["id"].as_str().unwrap();
    let capability = a["results"][1]["command"]["id"].as_str().unwrap();
    let first = store::board_read(&pool, &ws, "owner", None, None, None).await?;
    assert!(first["state"].get("domains").is_none());
    assert!(first.get("history").is_none());
    assert!(first.get("system_ids").is_none());
    let domain_key = format!("domain:{domain}");
    let branch = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            parent: Some(&domain_key),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(branch["items"][0]["id"], capability);
    let capability_key = format!("capability:{capability}");
    let branch = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            parent: Some(&capability_key),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(branch["items"][0]["title"], "Paid access persists");
    assert!(branch["items"][0].get("document").is_none());
    assert!(branch["items"][0].get("acs").is_none());
    let requirement_key = format!("requirement:{req}");
    let detail = store::board_read(
        &pool,
        &ws,
        "owner",
        Some("process"),
        Some(&requirement_key),
        None,
    )
    .await?;
    assert_eq!(detail["criteria"][0]["address"], format!("ac:{ac}"));
    let consumers = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            kind: Some("consumer"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(consumers["items"][0]["title"], "Buyer");
    assert!(
        store::board_read(&pool, &ws, "owner", Some("domain"), Some(domain), None)
            .await
            .is_err()
    );
    uuid::Uuid::parse_str(req)?;
    uuid::Uuid::parse_str(ac)?;
    assert!(
        a["results"][3]["system_ids"]["ac"][ac]
            .as_str()
            .unwrap()
            .starts_with(&format!("{prefix}-A-"))
    );
    let before = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    // Each failed package leaves neither a partial tree nor a consumed request ID.
    for bad in [
        json!({"op":"save_object","kind":"capability","revision":0,"parent":"domain:absent","data":{"title":"Broken","owner":"owner"},"reason":"Broken"}),
        json!({"op":"confirm_tree"}),
        json!({"op":"enroll_member","id":"intruder"}),
        json!({"op":"save_object","kind":"capability","revision":0,"parent":{"$ref":"future/address"},"data":{"title":"Broken","owner":"owner"},"reason":"Broken"}),
        json!({"op":"save_object","kind":"domain","revision":0,"parent":null,"data":{"title":"Broken","owner":"owner"},"reason":"Broken","actor":"owner"}),
        // A removed operation makes the whole package fail.
        json!({"op":"domain","title":"Broken","authority":"owner"}),
    ] {
        let ops = vec![
            operations[0].clone(),
            store::BatchOperation {
                reference: None,
                command: bad,
            },
        ];
        let error = store::execute_batch(&pool, &ws, "agent", "retry-after-error", &ops)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("operation 2"));
        assert_eq!(
            store::read(&pool, &ws, "owner", &store::Scope::full()).await?,
            before
        );
    }
    store::execute_batch(&pool, &ws, "agent", "retry-after-error", &operations).await?;
    let mut revision = a["results"][2]["command"].clone();
    revision["revision"] = json!(2);
    revision["data"]["title"] = json!("Access persists after signing in again");
    let updated = store::execute_input(&pool, &ws, "agent", "revise", &revision).await?;
    assert_eq!(updated["command"]["id"], req);
    assert_eq!(updated["system_ids"], a["results"][2]["system_ids"]);
    revision.as_object_mut().unwrap().remove("kind");
    assert!(
        store::execute_input(&pool, &ws, "agent", "missing-req", &revision)
            .await
            .is_err()
    );
    // Generated values persist as journal facts, and receipts cannot be rewritten.
    let after = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    store::migrate(&pool).await?;
    assert_eq!(
        store::read(&pool, &ws, "owner", &store::Scope::full()).await?,
        after
    );
    assert_eq!(
        store::execute_batch(&pool, &ws, "agent", "batch", &operations).await?["results"],
        a["results"]
    );
    let conn = pool.get().await?;
    assert!(
        conn.execute(
            "UPDATE joppa.command_receipts SET actor='other' WHERE workspace=$1",
            &[&ws]
        )
        .await
        .is_err()
    );
    let receipt: Value=serde_json::from_str(conn.query_one("SELECT result::text FROM joppa.command_receipts WHERE workspace=$1 AND request_id='batch'", &[&ws]).await?.get(0))?;
    assert_eq!(receipt["results"], a["results"]);
    conn.execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    assert!(store::backfill_board(&pool).await? >= 1);
    let domain_key = format!("domain:{domain}");
    assert_eq!(
        store::read_objects(
            &pool,
            &ws,
            "owner",
            &store::ObjectScope {
                address: Some(&domain_key),
                ..Default::default()
            }
        )
        .await?["document"]["data"]["title"],
        "Second area"
    );
    Ok(())
}

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
                "purpose":"implementation","contract":"Implement and provide evidence"},"reason":"Plan the work"})).await?;
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
