use anyhow::Result;
use joppa::{objects::TreeType, store};
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn generic_tasks_queue_priority_claim_race_history_and_replay() -> Result<()> {
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
    let ws = format!("generic-{}", uuid::Uuid::new_v4().simple());
    for (id, actor, cmd) in [
        (
            "workspace",
            "owner",
            json!({"op":"workspace","title":"Generic work"}),
        ),
        (
            "agent",
            "owner",
            json!({"op":"enroll_service","id":"agent","role":"agent"}),
        ),
        (
            "other",
            "owner",
            json!({"op":"enroll_service","id":"other","role":"agent"}),
        ),
    ] {
        store::execute_input(&pool, &ws, actor, id, &cmd).await?;
    }
    let current = store::tree_schema(&pool, &ws, "agent", None).await?;
    let mut types = std::collections::BTreeMap::new();
    for (kind, role, root, parents) in [
        ("spec", "document", true, vec!["spec"]),
        ("task", "task", false, vec!["*"]),
        ("work", "task", true, vec!["*"]),
    ] {
        types.insert(
            kind,
            TreeType {
                question: None,
                index: None,
                references: Default::default(),
                required_fields: Default::default(),
                parent_decision: false,
                status: None,
                status_order: None,
                process_fields: None,
                inbox: None,
                launch_blocker: None,
                color: None,
                title_size: None,
                badge: None,
                title_preview_length: None,
                title: kind.into(),
                role: role.into(),
                root,
                parents: parents.into_iter().map(String::from).collect(),
                parent_gate: None,
            },
        );
    }
    store::execute_remote_input(&pool,&ws,"owner","schema",&json!({"op":"set_tree_schema","revision":current["schema"]["revision"],"types":types,"reason":"Use specifications without REQ or AC"})).await?;
    for (id, parent) in [("root", None), ("urgent", Some("spec:root"))] {
        store::execute_remote_input(&pool,&ws,"agent",id,&json!({"op":"save_object","kind":"spec","id":id,"revision":0,"parent":parent,"data":{"title":id,"owner":"owner"},"reason":"Create a specification"})).await?;
    }
    let make = |parent: &str| json!({"op":"save_object","kind":"task","revision":0,"parent":parent,"data":{"title":"Implement the specification","owner":"owner","contract":"Implement the documented specification","purpose":"implementation","projects":[]},"reason":"Create work"});
    let first =
        store::execute_remote_input(&pool, &ws, "agent", "first", &make("spec:root")).await?;
    let second =
        store::execute_remote_input(&pool, &ws, "agent", "second", &make("spec:urgent")).await?;
    let second_address = second["object"]["address"].as_str().unwrap();
    let second_id = second["command"]["id"].as_str().unwrap();
    assert!(
        !second_id.contains('/'),
        "generated ids do not encode an AC or REQ"
    );
    let priority = vec!["spec:urgent".into(), "spec:root".into()];
    let page = store::task_queue_filtered(
        &pool,
        &ws,
        "agent",
        "available",
        1,
        None,
        None,
        None,
        &priority,
    )
    .await?;
    assert_eq!(page["tasks"][0]["address"], second_address);
    assert_eq!(page["tasks"][0]["parent"], "spec:urgent");
    assert_eq!(page["tasks"][0]["req"], Value::Null);
    let claim =
        json!({"op":"claim_task","id":second_address,"revision":page["tasks"][0]["revision"]});
    let (a, b) = tokio::join!(
        store::execute_remote_input(&pool, &ws, "agent", "claim-a", &claim),
        store::execute_remote_input(&pool, &ws, "other", "claim-b", &claim)
    );
    assert_ne!(a.is_ok(), b.is_ok(), "only one claimant succeeds");
    let actor = if a.is_ok() { "agent" } else { "other" };
    let (request, receipt) = if a.is_ok() {
        ("claim-a", a?)
    } else {
        ("claim-b", b?)
    };
    let retry = store::execute_remote_input(&pool, &ws, actor, request, &claim).await?;
    assert_eq!(retry["position"], receipt["position"]);
    assert_eq!(retry["replayed"], true);
    let next_actor = if actor == "agent" { "other" } else { "agent" };
    let rest = store::execute_remote_input(
        &pool,
        &ws,
        next_actor,
        "next",
        &json!({"op":"claim_next_task","priorities":priority}),
    )
    .await?;
    assert_eq!(rest["command"]["id"], first["object"]["address"]);
    assert!(rest["command"].get("req").is_none());
    let mine = store::task_queue(&pool, &ws, actor, "mine", 100, None).await?;
    let current = mine["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["address"] == second_address)
        .unwrap();
    store::execute_remote_input(&pool,&ws,actor,"complete",&json!({"op":"complete_task","id":second_address,"revision":current["revision"],"evidence":"The specified behavior works"})).await?;
    let system_id = second["system_ids"]["task"][second_id].as_str().unwrap();
    let read = store::read_item_scoped(
        &pool,
        system_id,
        "owner",
        store::Scope::from_args(&json!({}))?,
    )
    .await?;
    assert_eq!(read["process"]["status"], "done");
    let old = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            address: Some(second_address),
            revision: Some(1),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        old["document"]["data"]["contract"],
        "Implement the documented specification"
    );
    let conn = pool.get().await?;
    conn.execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    drop(conn);
    store::migrate(&pool).await?;
    let after = store::read_item_scoped(
        &pool,
        system_id,
        "owner",
        store::Scope::from_args(&json!({}))?,
    )
    .await?;
    assert_eq!(
        read, after,
        "journal replay preserves Task history and process"
    );
    assert_eq!(
        store::task_queue(&pool, &ws, "owner", "all", 100, None).await?["total"],
        2
    );
    let schema = store::tree_schema(&pool, &ws, "agent", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["ac"] = json!({"title":"Criterion","role":"ac","root":false,"parents":["spec"]});
    store::execute_remote_input(&pool,&ws,"owner","generic-ac-schema",&json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Permit criteria under specifications"})).await?;
    let ac=store::execute_remote_input(&pool,&ws,"agent","standalone-ac",&json!({"op":"save_object","kind":"ac","id":"standalone","revision":0,"parent":"spec:root","data":{"title":"Standalone criterion","owner":"owner","description":"Criterion under a specification"},"reason":"Use the configured parent"})).await?;
    let ac_id = ac["system_ids"]["ac"]["standalone"].as_str().unwrap();
    let selected =
        store::read_item_scoped(&pool, ac_id, "owner", store::Scope::from_args(&json!({}))?)
            .await?;
    assert_eq!(selected["document"]["parent"], "spec:root");
    assert_eq!(
        selected["document"]["data"]["title"],
        "Standalone criterion"
    );
    Ok(())
}

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
        "spec":{"title":"Specification","role":"requirement","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","parents":["spec"]},
        "work":{"title":"Work","role":"task","parents":["criterion"],"parent_gate":"ready"}
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
    let contract = |title: &str| json!({"title":title,"owner":"owner","contract":"Implement the agreed mechanism","purpose":"implementation"});
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
    write("agent", json!({"op":"complete_task","id":"work:a","revision":a["current_revision"],"evidence":"The source is complete"})).await?;
    let a = read("work:a").await?;
    assert_eq!(a["process"]["status"], "done");
    assert!(a["current_revision"].as_u64() > pins[0].as_u64());
    let b = read("work:b").await?;
    assert_eq!(b["review_required"], false);
    assert_eq!(b["work_ready"], true);
    assert_eq!(b["document"]["links"][0]["revision"], pins[0]);
    assert!(available().await?.contains(&"work:b".to_owned()));

    let c = read("work:c").await?;
    write("agent", json!({"op":"amend_task","id":"work:c","revision":c["current_revision"],"title":"Changed source","contract":"Implement the agreed mechanism","projects":[],"reason":"Change the source"})).await?;
    let d = read("work:d").await?;
    assert_eq!(d["review_required"], true);
    assert_eq!(d["document"]["links"][0]["revision"], pins[1]);
    assert!(!available().await?.contains(&"work:d".to_owned()));
    Ok(())
}
