// Part of tests/objects_postgres/part01.rs: the tail of the test before it, included right after it.

/// The replay, concurrency, ID and schema checks after the workspace setup.
async fn replay_concurrency_ids_and_schema_after_setup(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    restricted_url: &str,
) -> anyhow::Result<()> {
    let restricted = store::pool(restricted_url)?;
    assert_eq!(
        restricted
            .get()
            .await?
            .query_one("SELECT current_user", &[])
            .await?
            .get::<_, String>(0),
        "joppa_test_projection_guard"
    );
    let make = |body: &str, n: u32| json!({"op":"save_object","kind":"note","id":"source","revision":n,"parent":null,"data":{"title":"Source","owner":"owner","body":body},"links":[{"target":"domain:d","relation":"related"}],"reason":"Update source text"});
    let first = store::execute_remote_input(
        &restricted,
        ws,
        "agent",
        "note-create",
        &make("Original", 0),
    )
    .await?;
    assert!(first["system_ids"]["note"]["source"].as_str().is_some());
    pool.get()
        .await?
        .batch_execute("DROP TRIGGER test_projection_no_rewrite ON joppa.objects")
        .await?;

    let edit_a = make("A", 1);
    let edit_b = make("B", 1);
    let (a, b) = tokio::join!(
        store::execute_remote_input(&restricted, ws, "agent", "edit-a", &edit_a),
        store::execute_remote_input(&restricted, ws, "agent", "edit-b", &edit_b)
    );
    assert_ne!(a.is_ok(), b.is_ok(), "one stale writer must lose");
    let (id, body, result) = if a.is_ok() {
        ("edit-a", "A", a?)
    } else {
        ("edit-b", "B", b?)
    };
    let retry = store::execute_remote_input(pool, ws, "agent", id, &make(body, 1)).await?;
    assert_eq!(retry["position"], result["position"]);
    assert_eq!(retry["replayed"], true);
    assert!(
        store::execute_remote_input(pool, ws, "agent", id, &make("Changed retry", 1))
            .await
            .is_err()
    );
    let old = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            address: Some("note:source"),
            revision: Some(1),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(old["document"]["data"]["body"], "Original");
    assert_eq!(old["current_revision"], 2);
    assert_eq!(old["historical"], true);
    assert!(old["process"].is_null() && old["signal"].is_null());
    assert!(old.get("verification_summary").is_none());
    assert!(old["annotations"].as_object().unwrap().is_empty());
    assert!(old["link_groups"].as_array().unwrap().is_empty());
    assert_eq!(old["links"][0]["metadata_scope"], "current");

    assert!(
        old["document"]["at"]
            .as_str()
            .is_some_and(|v| !v.is_empty())
    );
    let public = first["system_ids"]["note"]["source"].as_str().unwrap();
    assert_eq!(
        resolve_id::resolve(pool, "agent", ws, public).await?,
        "note:source"
    );
    assert!(
        store::read_objects(pool, ws, "outsider", &ObjectScope::default())
            .await
            .is_err()
    );
    assert!(
        store::tree_schema(pool, ws, "outsider", None)
            .await
            .is_err()
    );
    let parent = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            address: Some("domain:d"),
            ..Default::default()
        },
    )
    .await?;
    let group = link_page::incoming_notes(pool, ws, "domain:d").await?;
    assert!(
        group["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["address"] == "note:source")
    );
    let listed = store::read_objects(pool, ws, "agent", &ObjectScope::default()).await?;
    let listed_parent = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["address"] == "domain:d")
        .unwrap();
    assert_eq!(
        parent["verification_summary"],
        listed_parent["verification_summary"]
    );
    assert_eq!(parent["verification_summary"]["total"], 1);
    let ops: Vec<store::BatchOperation> = serde_json::from_value(json!([
        {"ref":"one","command":{"op":"save_object","kind":"note","revision":0,"parent":null,"data":{"title":"Generated note","owner":"owner","body":"First"},"reason":"Create source"}},
        {"command":{"op":"save_object","kind":"note","id":{"$ref":"one/id"},"revision":{"$ref":"one/saved_revision"},"parent":null,"data":{"title":"Generated note","owner":"owner","body":"Second"},"reason":"Update source"}}
    ]))?;
    let batch = store::execute_remote_batch(pool, ws, "agent", "note-batch", &ops).await?;
    assert_eq!(batch["results"][1]["object"]["revision"], 2);
    let replay = store::execute_remote_batch(pool, ws, "agent", "note-batch", &ops).await?;
    assert_eq!(batch["results"], replay["results"]);
    store::execute_remote_input(pool,ws,"owner","default-schema",&json!({"op":"set_tree_schema","revision":store::tree_schema(pool,ws,"agent",None).await?["current_revision"],"types":joppa::objects::defaults(),"reason":"Record the default schema"})).await?;
    let first_view: String = pool
        .get()
        .await?
        .query_one(
            "SELECT first_view::text FROM joppa.board_cache WHERE workspace=$1",
            &[&ws],
        )
        .await?
        .get(0);
    assert_eq!(
        serde_json::from_str::<Value>(&first_view)?["custom_tree"],
        false
    );
    let mut types = joppa::objects::defaults();
    types.insert(
        "initiative".into(),
        joppa::objects::TreeType {
            question: None,
            index: None,
            references: Default::default(),
            required_fields: Default::default(),
            parent_decision: false,
            status: Some("none".into()),
            status_order: None,
            process_fields: None,
            inbox: None,
            launch_blocker: None,
            color: None,
            title_size: None,
            badge: None,
            title_preview_length: None,
            parent_gate: None,
            title: "Initiative".into(),
            role: "group".into(),
            root: true,
            parents: vec![],
        },
    );
    types
        .get_mut("requirement")
        .unwrap()
        .parents
        .push("initiative".into());
    store::execute_remote_input(
        pool,
        ws,
        "owner",
        "schema",
        &json!({"op":"set_tree_schema","revision":store::tree_schema(pool,ws,"agent",None).await?["current_revision"],"types":types,"reason":"Use an initiative"}),
    )
    .await?;
    store::execute_remote_input(pool,ws,"agent","group",&json!({"op":"save_object","kind":"initiative","id":"release","revision":0,"parent":null,"data":{"title":"Release","owner":"owner"},"reason":"Group release work"})).await?;
    store::execute_remote_input(pool,ws,"agent","req",&json!({"op":"save_object","kind":"requirement","id":"custom","revision":0,"parent":"initiative:release","data":{"title":"Open the release","body":"Show the current release","owner":"owner","consumers":["customer"]},"reason":"Define the release"})).await?;
    let children = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            parent: Some("initiative:release"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(children["items"][0]["address"], "requirement:custom");
    let state = store::read(pool, ws, "owner", &store::Scope::full()).await?;
    assert_eq!(
        state["state"]["objects"]["requirement:custom"]["revisions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let count: Value = state["state"]["objects"].clone();
    store::migrate(pool).await?;
    assert_eq!(
        store::read(pool, ws, "owner", &store::Scope::full()).await?["state"]["objects"],
        count
    );
    assert_eq!(
        store::tree_schema(pool, ws, "agent", None).await?["current_revision"],
        // migrate_consumers adds one revision: one consumer per criterion. migrate_test_rules
        // adds one more: the test rules and the status rule of each type. migrate_note_parents
        // adds one more: an inbox type has no parent.
        9
    );
    assert_eq!(
        store::tree_schema(pool, ws, "agent", Some(0)).await?["schema"]["revision"],
        0
    );
    let found =
        store::search::find(pool, "agent", ws, "Generated", Some("note"), 10, None).await?;
    assert!(
        found["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "note")
    );
    Ok(())
}
