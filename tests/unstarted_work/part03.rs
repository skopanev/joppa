// Part 3 of tests/unstarted_work.rs, included there, so the test names stay unchanged.

/// Fixture 1: criterion A is ready, has valid dependencies, no green recorded status and no
/// planning claim. L (`agent`) holds T at revision 4 and blocks it. Fixture 2 adds Task V
/// under A in progress for `worker`.
async fn blocked_fixture(with_v: bool) -> anyhow::Result<(Pool, String)> {
    let (pool, ws) = fixture().await?;
    apply(
        &pool,
        &ws,
        "owner",
        json!({"op":"workspace","title":"Blocked work"}),
    )
    .await?;
    for actor in ["agent", "worker"] {
        apply(
            &pool,
            &ws,
            "owner",
            json!({"op":"enroll_service","id":actor,"role":"agent"}),
        )
        .await?;
    }
    let schema = store::tree_schema(&pool, &ws, "owner", None).await?;
    apply(&pool, &ws, "owner", json!({"op":"set_tree_schema","revision":schema["current_revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    let work = |id: &str| json!({"op":"save_object","kind":"work","id":id,"revision":0,"parent":"criterion:a","data":{"title":id,"owner":"owner","contract":"Implement this criterion","purpose":"implementation"},"reason":"Set the work"});
    for command in [
        json!({"op":"save_object","kind":"spec","id":"one","revision":0,"parent":null,"data":{"title":"One rule","body":"Implement the agreed rule","owner":"owner"},"reason":"Set the rule"}),
        json!({"op":"save_object","kind":"criterion","id":"a","revision":0,"parent":"spec:one","data":{"title":"A","owner":"owner"},"reason":"Set the criterion"}),
    ] {
        apply(&pool, &ws, "owner", command).await?;
    }
    let spec = task_detail(&pool, &ws, "spec:one").await?;
    apply(
        &pool,
        &ws,
        "owner",
        json!({"op":"confirm","req":"spec:one","revision":spec["process"]["revision"]}),
    )
    .await?;
    assess_current(&pool, &ws, "criterion:a", true).await?;
    apply(&pool, &ws, "owner", work("t")).await?;
    for (op, extra) in [
        ("claim_task", json!({})),
        ("release_task", json!({"reason":"Return the Task"})),
        ("claim_task", json!({})),
    ] {
        let revision = task_detail(&pool, &ws, "work:t").await?["current_revision"].clone();
        let mut command = json!({"op":op,"id":"work:t","revision":revision});
        command
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        apply(&pool, &ws, "agent", command).await?;
    }
    assert_eq!(
        task_detail(&pool, &ws, "work:t").await?["current_revision"],
        4
    );
    if with_v {
        apply(&pool, &ws, "owner", work("v")).await?;
        apply(
            &pool,
            &ws,
            "worker",
            json!({"op":"claim_task","id":"work:v","revision":1}),
        )
        .await?;
    }
    apply(&pool, &ws, "agent", json!({"op":"block_task","id":"work:t","revision":4,"reason":"Verification failed","evidence":"test x failed on abc123"})).await?;
    let a = task_detail(&pool, &ws, "criterion:a").await?;
    assert_eq!(a["process"]["assessment"]["state"], "ready");
    assert!(a["work_ready"] == true && a["process"]["planning_claim"].is_null());
    assert_ne!(a["process"]["verification"], "pass");
    Ok((pool, ws))
}

// failed-task-blocks-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_blocked_task_does_not_keep_its_criterion_out_of_unstarted_work() -> anyhow::Result<()> {
    let (pool, ws) = blocked_fixture(false).await?;
    let page = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&page), vec!["criterion:a"]);
    assert_eq!(page["items"][0]["work_summary"]["active"], 0);

    // Fixture 2: ordinary work in progress keeps A out; planning still claims A.
    let (pool, ws) = blocked_fixture(true).await?;
    let page = candidates(&pool, &ws).await?;
    assert!(criteria(&page).is_empty(), "{page}");
    let a = task_detail(&pool, &ws, "criterion:a").await?;
    apply(
        &pool,
        &ws,
        "agent",
        json!({"op":"claim_planning","ac":"criterion:a","revision":a["current_revision"]}),
    )
    .await?;
    assert_eq!(
        task_detail(&pool, &ws, "criterion:a").await?["process"]["planning_claim"]["actor"],
        "agent"
    );

    // Fixture 2 without a planning claim: A returns after V ends.
    let (pool, ws) = blocked_fixture(true).await?;
    apply(&pool, &ws, "worker", json!({"op":"complete_task","commit":"c0ffee0","id":"work:v","revision":2,"evidence":"The ordinary work is complete"})).await?;
    let page = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&page), vec!["criterion:a"]);
    store::check_journal(&pool).await?;
    Ok(())
}

// failed-task-blocks-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_workspace_owner_retires_a_blocked_task_without_a_planning_claim() -> anyhow::Result<()>
{
    // Fixture 5: T is blocked at revision 5 and A has no planning claim.
    let (pool, ws) = blocked_fixture(false).await?;
    apply(&pool, &ws, "owner", json!({"op":"retire_task","id":"work:t","revision":5,"reason":"The blocked work is no longer required"})).await?;
    assert_eq!(
        task_detail(&pool, &ws, "work:t").await?["process"]["status"],
        "retired"
    );
    let block = store::read_objects(
        &pool,
        &ws,
        "owner",
        &ObjectScope {
            address: Some("work:t"),
            revision: Some(5),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(block["document"]["reason"], "Verification failed");
    let page = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&page), vec!["criterion:a"]);
    assert!(task_detail(&pool, &ws, "criterion:a").await?["process"]["planning_claim"].is_null());
    store::check_journal(&pool).await?;
    Ok(())
}
