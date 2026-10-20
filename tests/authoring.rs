mod common;
use anyhow::Result;
use joppa::store;
use serde_json::{Value, json};

#[test]
fn agents_edit_draft_titles_without_taking_decision_authority() {
    let mut w = common::World::new();
    w.ok(
        "agent",
        json!({"op":"domain","id":"draft","title":"Draft area","authority":"owner"}),
    );
    w.ok(
        "agent",
        json!({"op":"capability","id":"draft-cap","title":"Draft ability","domain":"draft"}),
    );
    let edit = json!({"op":"edit_domain","id":"draft","title":"Clear area","authority":"owner","reason":"Clarify wording"});
    let cap = json!({"op":"edit_capability","id":"draft-cap","title":"Clear ability","reason":"Clarify wording"});
    w.ok("agent", edit.clone());
    w.ok("agent", cap.clone());
    assert!(!w.state.domains["draft"].confirmed);
    assert!(!w.state.capabilities["draft-cap"].confirmed);
    let mut reassign = edit.clone();
    reassign["authority"] = json!("legal");
    w.no("agent", reassign);
    w.no("runner", edit.clone());
    w.no("runner", cap.clone());
    w.no("agent", json!({"op":"confirm_tree"}));
    w.ok("owner", json!({"op":"confirm_tree"}));
    w.no("agent", edit.clone());
    w.no("agent", cap.clone());
    w.ok("owner", edit);
    w.ok("owner", cap);
    w.ok(
        "agent",
        json!({"op":"domain","id":"archived","title":"Draft","authority":"owner"}),
    );
    w.ok(
        "agent",
        json!({"op":"capability","id":"archived-cap","title":"Draft ability","domain":"archived"}),
    );
    w.ok(
        "owner",
        json!({"op":"set_domain_archived","id":"archived","archived":true,"reason":"Not in scope"}),
    );
    w.no("agent", json!({"op":"edit_domain","id":"archived","title":"Changed","authority":"owner","reason":"Change"}));
    w.no(
        "agent",
        json!({"op":"edit_capability","id":"archived-cap","title":"Changed","reason":"Change"}),
    );
}

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
        &json!({"op":"consumer","id":"buyer","title":"Buyer","owner":"owner"}),
    )
    .await?;
    let input = json!({"op":"domain","title":"Area","authority":"owner"});
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
    assert!(public_id.starts_with(&format!("{ws}-D-")));
    assert_eq!(
        store::read_item(&pool, public_id, "owner").await?["selection"]["id"],
        id
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
            &json!({"op":"domain","title":"Changed","authority":"owner"})
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
        {"ref":"area","command":{"op":"domain","title":"Second area","authority":"owner"}},
        {"ref":"ability","command":{"op":"capability","title":"Keep paid access","domain":{"$ref":"area/id"}}},
        {"ref":"requirement","command":{"op":"revise","expected_revision":0,"capability":{"$ref":"ability/id"},"consumers":["buyer"],"title":"Paid access persists","body":"Only iOS","owner":"owner","reason":"Agreed scope","acs":[{"text":"On iOS access survives sign-in"}]}},
        {"command":{"op":"comment","req":{"$ref":"requirement/req"},"revision":1,"text":"Please review","kind":"comment","ac":{"$ref":"requirement/acs/0/id"}}}
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
        a["results"][1]["command"]["domain"],
        a["results"][0]["command"]["id"]
    );
    let req = a["results"][2]["command"]["req"].as_str().unwrap();
    let ac = a["results"][2]["command"]["acs"][0]["id"].as_str().unwrap();
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
    assert_eq!(detail["criteria"][0]["id"], ac);
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
        a["results"][2]["system_ids"]["ac"][ac]
            .as_str()
            .unwrap()
            .starts_with(&format!("{ws}-A-"))
    );
    let before = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    // Each failed package leaves neither a partial tree nor a consumed request ID.
    for bad in [
        json!({"op":"capability","title":"Broken","domain":"absent"}),
        json!({"op":"confirm_tree"}),
        json!({"op":"enroll_member","id":"intruder"}),
        json!({"op":"capability","title":"Broken","domain":{"$ref":"future/id"}}),
        json!({"op":"domain","title":"Broken","authority":"owner","actor":"owner"}),
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
    revision["expected_revision"] = json!(1);
    revision["title"] = json!("Access persists after signing in again");
    let updated = store::execute_input(&pool, &ws, "agent", "revise", &revision).await?;
    assert_eq!(updated["command"]["req"], req);
    assert_eq!(updated["system_ids"], a["results"][2]["system_ids"]);
    revision.as_object_mut().unwrap().remove("req");
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
        &json!({"op":"consumer","id":"buyer","title":"Buyer","owner":"owner"}),
    )
    .await?;
    for (suffix, domain) in [("old", "ordinary"), ("new", "urgent")] {
        store::execute_input(
            &pool,
            &ws,
            "owner",
            &format!("domain-{suffix}"),
            &json!({"op":"domain","id":domain,"title":domain,"authority":"owner"}),
        )
        .await?;
        store::execute_input(
            &pool,
            &ws,
            "owner",
            &format!("cap-{suffix}"),
            &json!({"op":"capability","id":format!("cap-{suffix}"),"title":suffix,"domain":domain}),
        )
        .await?;
        store::execute_input(
            &pool,
            &ws,
            "owner",
            &format!("req-{suffix}"),
            &json!({"op":"revise","req":format!("req-{suffix}"),"expected_revision":0,
                "capability":format!("cap-{suffix}"),"consumers":["buyer"],"title":suffix,
                "body":"Scope","owner":"owner","reason":"Start",
                "acs":[{"id":format!("ac-{suffix}"),"text":"Works"}]}),
        )
        .await?;
        store::execute_input(&pool, &ws, "owner", &format!("assess-{suffix}"),
            &json!({"op":"assess","req":format!("req-{suffix}"),"revision":1,
                "ac":format!("ac-{suffix}"),"assessment":{"ready":true,"reason":"Ready","owner":null,"decision":null}})).await?;
        store::execute_input(&pool, &ws, "owner", &format!("task-{suffix}"),
            &json!({"op":"task","req":format!("req-{suffix}"),"revision":1,"id":format!("task-{suffix}"),
                "title":suffix,"acs":[format!("ac-{suffix}")],"projects":[],
                "purpose":"implementation","contract":"Implement and provide evidence"})).await?;
    }
    let original = store::task_queue(&pool, &ws, "owner", "all", 50, None).await?;
    assert_eq!(original["workspace"], public_ws);
    let created_at = original["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == "task:req-old/task-old")
        .unwrap()["created_at"]
        .as_u64()
        .unwrap();
    assert!(created_at > 0);
    pool.get().await?.execute(
        "UPDATE joppa.task_queue SET created_at=0,data=jsonb_set(data,'{created_at}','0'::jsonb) WHERE workspace=$1 AND address='task:req-old/task-old'",
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
            .find(|task| task["id"] == "task:req-old/task-old")
            .unwrap()["created_at"],
        created_at
    );
    let canonical_priorities = vec!["task:req-new/task-new".to_owned()];
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
    assert_eq!(canonical_preview["tasks"][0]["id"], "task:req-new/task-new");
    let canonical_claim = store::execute_input(
        &pool,
        &ws,
        "owner",
        "canonical-next",
        &json!({"op":"claim_next_task","priorities":canonical_priorities}),
    )
    .await?;
    assert_eq!(canonical_claim["command"]["req"], Value::Null);
    assert_eq!(canonical_claim["command"]["id"], "task:req-new/task-new");
    store::execute_input(&pool,&ws,"owner","release-canonical",&json!({"op":"release_task","req":"req-new","revision":1,"id":"task-new","reason":"Return the canonical priority task"})).await?;
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
    assert_eq!(preview["tasks"][0]["id"], "task:req-new/task-new");
    assert_eq!(
        store::task_queue(&pool, &ws, "owner", "available", 50, None).await?["total"],
        2
    );
    let request = json!({"op":"claim_next_task","priorities":priorities});
    let first = store::execute_input(&pool, &ws, "owner", "next-1", &request).await?;
    assert_eq!(first["command"]["id"], "task:req-new/task-new");
    let second = store::execute_input(&pool, &ws, "owner", "next-2", &request).await?;
    assert_eq!(second["command"]["id"], "task:req-old/task-old");
    store::execute_input(&pool, &ws, "owner", "release-old",
        &json!({"op":"release_task","req":"req-old","revision":1,"id":"task-old","reason":"Return"})).await?;
    let fallback = store::execute_input(
        &pool,
        &ws,
        "owner",
        "next-fallback",
        &json!({"op":"claim_next_task","priorities":["domain:urgent"]}),
    )
    .await?;
    assert_eq!(fallback["command"]["id"], "task:req-old/task-old");
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
async fn task_carry_marker_survives_journal_replay_and_new_tasks_have_one_ac() -> Result<()> {
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
            json!({"op":"consumer","id":"buyer","title":"Buyer","owner":"owner"}),
        ),
        (
            "domain",
            "agent",
            json!({"op":"domain","id":"area","title":"Area","authority":"owner"}),
        ),
        (
            "capability",
            "agent",
            json!({"op":"capability","id":"cap","title":"Ability","domain":"area"}),
        ),
        (
            "revise-1",
            "agent",
            json!({"op":"revise","req":"req","expected_revision":0,"capability":"cap","consumers":["buyer"],"title":"Requirement","body":"Scope","owner":"owner","reason":"Start","acs":[{"id":"ac","text":"Works"}]}),
        ),
    ] {
        store::execute_input(&pool, &ws, actor, id, &command).await?;
    }
    let multi = json!({"op":"task","req":"req","revision":1,"id":"bad","title":"Bad","acs":["ac","ac"],"projects":[],"purpose":"decision","assignee":"agent","contract":"Record the specialist decision"});
    assert!(
        store::execute_input(&pool, &ws, "agent", "multi", &multi)
            .await
            .is_err()
    );
    assert!(store::execute_input(&pool,&ws,"agent","no-contract",&json!({"op":"task","req":"req","revision":1,"id":"no-contract","title":"Decide","acs":["ac"],"projects":[],"purpose":"decision","assignee":"agent"})).await.is_err());
    let task = json!({"op":"task","req":"req","revision":1,"id":"task","title":"Decide","acs":["ac"],"projects":[],"purpose":"decision","contract":"Record the specialist decision"});
    store::execute_input(&pool, &ws, "agent", "task", &task).await?;
    let revise = json!({"op":"revise","req":"req","expected_revision":1,"capability":"cap","consumers":["buyer"],"title":"Requirement revised","body":"Scope","owner":"owner","reason":"Clarified","acs":[{"id":"ac","text":"Works better"}]});
    let written = store::execute_input(&pool, &ws, "agent", "revise-2", &revise).await?;
    assert_eq!(written["command"]["carry_tasks"], true);
    assert_eq!(written["command"]["coverage_policy"], true);
    assert_eq!(
        store::read_objects(
            &pool,
            &ws,
            "owner",
            &store::ObjectScope {
                address: Some("task:req/task"),
                ..Default::default()
            }
        )
        .await?["process"]["review_required"],
        true
    );
    store::execute_input(&pool, &ws, "owner", "retire", &json!({"op":"retire_task","req":"req","revision":2,"id":"task","reason":"Decision changed"})).await?;
    let after = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    assert_eq!(
        after["state"]["task_process"]["task:req/task"]["retired"],
        "Decision changed"
    );
    store::migrate(&pool).await?;
    assert_eq!(
        store::read(&pool, &ws, "owner", &store::Scope::full()).await?,
        after
    );
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "wrong-project",
        &json!({"op":"project","id":"wrong","repository":"https://example.test/wrong"}),
    )
    .await?;
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "right-project",
        &json!({"op":"project","id":"mobileapps","repository":"https://example.test/mobileapps"}),
    )
    .await?;
    let open = json!({"op":"task","req":"req","revision":2,"id":"open-task","title":"Build access flow","acs":["ac"],"projects":["wrong"],"purpose":"implementation","contract":"Implement the revised access flow and attach merged build evidence"});
    store::execute_input(&pool, &ws, "agent", "open-task", &open).await?;
    store::execute_input(&pool,&ws,"agent","amend-project",&json!({"op":"amend_task","req":"req","revision":2,"id":"open-task","title":"Build access flow","contract":"Implement the revised mobile access flow and attach merged build evidence","projects":["mobileapps"],"reason":"Correct project identifier before claim"})).await?;
    assert_eq!(
        store::task_queue(&pool, &ws, "agent", "waiting", 50, None).await?["total"],
        1
    );
    assert_eq!(
        store::task_queue(&pool, &ws, "agent", "available", 50, None).await?["total"],
        0
    );
    store::execute_input(&pool,&ws,"agent","ready",&json!({"op":"assess","req":"req","revision":2,"ac":"ac","assessment":{"ready":true,"reason":"Contract is clear","owner":null,"decision":null}})).await?;
    let queue = store::task_queue(&pool, &ws, "agent", "available", 1, None).await?;
    assert_eq!(queue["tasks"][0]["id"], "task:req/open-task");
    assert_eq!(queue["tasks"][0]["ac"], "ac");
    assert_eq!(queue["tasks"][0]["projects"], json!(["mobileapps"]));
    let page = store::task_queue(&pool, &ws, "agent", "all", 1, None).await?;
    let cursor = page["next_cursor"]
        .as_str()
        .expect("second Task page")
        .to_owned();
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
    assert_eq!(success["command"]["id"], "task:req/open-task");
    assert!(
        store::task_queue(&pool, &ws, "agent", "all", 1, Some(&cursor))
            .await
            .is_err()
    );
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
        &json!({"op":"release_task","req":"req","revision":2,"id":"open-task","reason":"Handoff"}),
    )
    .await?;
    store::execute_input(
        &pool,
        &ws,
        loser,
        "claim-explicit",
        &json!({"op":"claim_task","req":"req","revision":2,"id":"open-task"}),
    )
    .await?;
    store::execute_input(&pool,&ws,loser,"complete",&json!({"op":"complete_task","req":"req","revision":2,"id":"open-task","evidence":"merged build abc"})).await?;
    assert_eq!(
        store::task_queue(&pool, &ws, loser, "mine", 50, None).await?["total"],
        0
    );
    assert_eq!(
        store::task_queue(&pool, &ws, loser, "all", 50, None).await?["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == "task:req/open-task")
            .unwrap()["status"],
        "done"
    );
    Ok(())
}
