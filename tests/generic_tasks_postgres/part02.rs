// Part 2 of tests/generic_tasks_postgres.rs, included there, so the test names stay unchanged.

// [ac:pins-ignore-process-events-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn separate_claim_and_completion_writes_keep_a_dependency_pin_current() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let name: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("pins-{}", uuid::Uuid::new_v4().simple());
    let write = |actor: &'static str, cmd: Value| {
        let (pool, ws) = (pool.clone(), ws.clone());
        async move {
            if cmd["op"] == "enroll_service" {
                let (id, role) = (
                    cmd["id"].as_str().unwrap_or_default(),
                    cmd["role"].as_str().unwrap_or_default(),
                );
                return store::enroll_service(&pool, &ws, actor, id, role).await;
            }
            store::execute_remote_input(&pool, &ws, actor, &uuid::Uuid::new_v4().to_string(), &cmd)
                .await
        }
    };
    let read = |address: &'static str| {
        let (pool, ws) = (pool.clone(), ws.clone());
        async move {
            store::read_objects(
                &pool,
                &ws,
                "owner",
                &store::ObjectScope {
                    address: Some(address),
                    ..Default::default()
                },
            )
            .await
        }
    };
    let available = || {
        let (pool, ws) = (pool.clone(), ws.clone());
        async move {
            let page = store::task_queue(&pool, &ws, "agent", "available", 100, None).await?;
            anyhow::Ok(
                page["tasks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t["address"].as_str().unwrap().to_owned())
                    .collect::<Vec<_>>(),
            )
        }
    };
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Dependency pins"}),
    )
    .await?;
    write(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(&pool, &ws, "owner", None).await?;
    write("owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure ready criteria","types":{
        "spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    write("agent", json!({"op":"save_object","kind":"spec","id":"one","revision":0,"parent":null,"data":{"title":"One rule","body":"Implement the agreed rule","owner":"owner"},"reason":"Set the rule"})).await?;
    write("agent", json!({"op":"save_object","kind":"criterion","id":"a","revision":0,"parent":"spec:one","data":{"title":"a","owner":"owner"},"reason":"Set the criterion"})).await?;
    let revision = read("spec:one").await?["process"]["revision"].clone();
    write(
        "owner",
        json!({"op":"confirm","req":"spec:one","revision":revision}),
    )
    .await?;
    write("agent", json!({"op":"assess","req":"spec:one","revision":revision,"ac":"criterion:a","assessment":{"ready":true,"reason":"Known scope"}})).await?;
    let contract = |title: &str| json!({"title":title,"owner":"owner","body":"Implement the agreed mechanism","purpose":"implementation"});
    let mut pins = Vec::new();
    for (source, dependent) in [("a", "b"), ("c", "d")] {
        write("agent", json!({"op":"save_object","kind":"work","id":source,"revision":0,"parent":"criterion:a","data":contract(source),"reason":"Set the work contract"})).await?;
        let pin = read(if source == "a" { "work:a" } else { "work:c" }).await?["current_revision"]
            .clone();
        write("agent", json!({"op":"save_object","kind":"work","id":dependent,"revision":0,"parent":"criterion:a","data":contract(dependent),"links":[{"target":format!("work:{source}"),"relation":"depends_on","revision":pin,"blocking":true}],"reason":"Depend on the source"})).await?;
        pins.push(pin);
    }
    let a = read("work:a").await?;
    let b = read("work:b").await?;
    let criterion = read("criterion:a").await?;
    assert_eq!(criterion["process"]["assessment"]["state"], "ready");
    assert!(criterion["process"]["readiness_blocker"].is_null());
    assert_eq!(a["work_ready"], true);
    assert_eq!(b["review_required"], false);
    assert_eq!(b["work_ready"], false);
    let open = available().await?;
    assert!(open.contains(&"work:a".to_owned()));
    assert!(!open.contains(&"work:b".to_owned()));

    write(
        "agent",
        json!({"op":"claim_task","id":"work:a","revision":a["current_revision"]}),
    )
    .await?;
    let a = read("work:a").await?;
    assert_eq!(a["process"]["status"], "in_progress");
    assert_eq!(read("work:b").await?["review_required"], false);
    write("agent", json!({"op":"complete_task","commit":"c0ffee0","id":"work:a","revision":a["current_revision"],"evidence":"The source is complete"})).await?;
    let a = read("work:a").await?;
    assert_eq!(a["process"]["status"], "done");
    assert!(a["current_revision"].as_u64() > pins[0].as_u64());
    let b = read("work:b").await?;
    assert_eq!(b["review_required"], false);
    assert_eq!(b["work_ready"], true);
    assert_eq!(b["document"]["links"][0]["revision"], pins[0]);
    assert!(available().await?.contains(&"work:b".to_owned()));

    let c = read("work:c").await?;
    let mut data = c["document"]["data"].clone();
    data["title"] = json!("Changed source");
    data["body"] = json!("Implement the agreed mechanism");
    write("agent", json!({"op":"save_object","kind":"work","id":"c","revision":c["current_revision"],"parent":c["document"]["parent"],"data":data,"links":c["document"]["links"],"reason":"Change the source"})).await?;
    let d = read("work:d").await?;
    assert_eq!(d["review_required"], true);
    assert_eq!(d["document"]["links"][0]["revision"], pins[1]);
    assert!(!available().await?.contains(&"work:d".to_owned()));
    Ok(())
}
