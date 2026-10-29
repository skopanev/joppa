// Part 2 of tests/unstarted_work.rs, included there, so the test names stay unchanged.

// [ac:unstarted-work-visible-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn custom_planning_uses_roles_nested_task_bindings_and_current_dependencies()
-> anyhow::Result<()> {
    let (pool, ws) = fixture().await?;
    apply(
        &pool,
        &ws,
        "owner",
        json!({"op":"workspace","title":"Custom planning"}),
    )
    .await?;
    apply(
        &pool,
        &ws,
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(&pool, &ws, "agent", None).await?;
    let types = json!({"spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":["*"]},"condition":{"title":"Condition","role":"ac","status":"tests","root":false,"parents":["*"]},"section":{"title":"Section","role":"group","status":"children","root":true,"parents":["*"]},"job":{"title":"Job","role":"task","status":"none","root":false,"parents":["*"],"parent_gate":"ready"},"input":{"title":"Input","role":"document","status":"none","inbox":true,"root":true,"parents":[]}});
    apply(&pool,&ws,"owner",json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Use arbitrary type names"})).await?;
    for (kind, id, parent, data) in [
        (
            "spec",
            "s",
            None,
            json!({"title":"Spec","owner":"owner","body":"The exact contract"}),
        ),
        (
            "condition",
            "a",
            Some("spec:s"),
            json!({"title":"First condition","owner":"owner"}),
        ),
        (
            "condition",
            "b",
            Some("spec:s"),
            json!({"title":"Second condition","owner":"owner","depends_on":["condition:a"]}),
        ),
        (
            "section",
            "nested",
            Some("condition:a"),
            json!({"title":"Nested work","owner":"owner"}),
        ),
        (
            "section",
            "blocker",
            None,
            json!({"title":"Unverified dependency","owner":"owner"}),
        ),
        (
            "input",
            "raw",
            None,
            json!({"title":"Unrelated source","owner":"owner","body":"raw input ".repeat(100)}),
        ),
    ] {
        apply(&pool,&ws,"agent",json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"links":[],"reason":"Set up the planning fixture"})).await?;
    }
    assess_current(&pool, &ws, "condition:a", false).await?;
    assess_current(&pool, &ws, "condition:b", true).await?;
    assert!(
        criteria(&candidates(&pool, &ws).await?).is_empty(),
        "waiting prerequisite must exclude the dependent criterion"
    );
    assess_current(&pool, &ws, "condition:a", true).await?;
    let page = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&page), vec!["condition:a", "condition:b"]);
    assert!(!page.to_string().contains("raw input"));
    for item in page["items"].as_array().unwrap() {
        let source = task_detail(&pool, &ws, item["address"].as_str().unwrap()).await?;
        assert_eq!(item["revision"], source["current_revision"]);
        assert_eq!(
            item["signal"]["assessment"],
            source["process"]["assessment"]
        );
    }
    let ordinary_before = store::read_objects(&pool, &ws, "agent", &ObjectScope::default()).await?;
    let spec = ordinary_before["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["address"] == "spec:s")
        .unwrap();
    assert_eq!(
        spec["verification_summary"],
        json!({"total":2,"passed":0,"failed":0})
    );
    let unrelated = ordinary_before["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["address"] == "input:raw")
        .unwrap();
    // The wildcard schema permits criteria below this type, but it has no children.
    assert_eq!(
        unrelated["verification_summary"],
        json!({"total":0,"passed":0,"failed":0})
    );
    assert!(
        ordinary_before["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["parent"].is_null() && item.get("work_summary").is_none())
    );
    apply(&pool,&ws,"agent",json!({"op":"save_object","kind":"job","id":"spec-work","revision":0,"parent":"spec:s","data":{"title":"Work on the specification","owner":"owner","contract":"Work on the specification as a whole","purpose":"implementation"},"reason":"Check the specification binding boundary"})).await?;
    assert_eq!(
        criteria(&candidates(&pool, &ws).await?),
        vec!["condition:a", "condition:b"]
    );
    assert!(
        candidates(&pool, &ws).await?["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["work_summary"] == json!({"active":0,"done":0,"retired":0}))
    );
    let ordinary_after = store::read_objects(&pool, &ws, "agent", &ObjectScope::default()).await?;
    let mut expected = ordinary_before["items"].clone();
    // The wildcard schema also permits a criterion below the new direct work child.
    let spec = expected
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["address"] == "spec:s")
        .unwrap();
    spec["verification_summary"] = json!({"total":3,"passed":0,"failed":0});
    assert_eq!(expected, ordinary_after["items"]);
    let direct = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            work_filter: Some("unstarted"),
            parent: Some("spec:s"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(criteria(&direct), vec!["condition:a", "condition:b"]);
    apply(&pool,&ws,"agent",json!({"op":"save_object","kind":"job","id":"one","revision":0,"parent":"section:nested","data":{"title":"Implement the first condition","owner":"owner","contract":"Implement this exact condition","purpose":"implementation"},"reason":"Bind work through an intermediate group"})).await?;
    assert_eq!(
        criteria(&candidates(&pool, &ws).await?),
        vec!["condition:b"]
    );
    let queue = store::task_queue(&pool, &ws, "agent", "available", 100, None).await?;
    assert!(
        queue["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["address"] == "job:one")
    );
    let b = task_detail(&pool, &ws, "condition:b").await?;
    apply(&pool,&ws,"agent",json!({"op":"save_object","kind":"condition","id":"b","revision":b["current_revision"],"parent":b["document"]["parent"],"data":b["document"]["data"],"links":[{"target":"section:blocker","relation":"depends_on","revision":1,"blocking":true}],"reason":"Require an unmet dependency"})).await?;
    assess_current(&pool, &ws, "condition:a", true).await?;
    assess_current(&pool, &ws, "condition:b", true).await?;
    assert!(criteria(&candidates(&pool, &ws).await?).is_empty());
    let b = task_detail(&pool, &ws, "condition:b").await?;
    assert_eq!(b["work_ready"], false);
    apply(&pool,&ws,"agent",json!({"op":"save_object","kind":"condition","id":"b","revision":b["current_revision"],"parent":b["document"]["parent"],"data":b["document"]["data"],"links":[],"reason":"Remove the fixture dependency"})).await?;
    assess_current(&pool, &ws, "condition:a", true).await?;
    assess_current(&pool, &ws, "condition:b", true).await?;
    let task = task_detail(&pool, &ws, "job:one").await?;
    apply(&pool,&ws,"owner",json!({"op":"retire_task","id":"job:one","revision":task["current_revision"],"reason":"Keep the fixture history"})).await?;
    let retired_page = candidates(&pool, &ws).await?;
    assert_eq!(criteria(&retired_page).len(), 2);
    assert_eq!(
        retired_page["items"][0]["work_summary"],
        json!({"active":0,"done":0,"retired":1})
    );
    assert_eq!(
        task_detail(&pool, &ws, "job:one").await?["process"]["status"],
        "retired"
    );
    Ok(())
}
