// Part of tests/objects_postgres/part02.rs: the tail of the test before it, included right after it.

/// The note queue, link and colour checks after the workspace setup.
async fn note_queue_links_and_color_after_setup(
    pool: &deadpool_postgres::Pool,
    ws: &str,
) -> anyhow::Result<()> {
    for (id, parent, links) in [
        (
            "a",
            None::<&str>,
            json!([{"target":"domain:d","relation":"related"}]),
        ),
        ("b", None, json!([])),
        // An inbox input has no parent: note c links to the domain that it informs.
        (
            "c",
            None,
            json!([{"target":"domain:d","relation":"related"}]),
        ),
    ] {
        store::execute_remote_input(pool,ws,"agent",id,&json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":parent,"data":{"title":id,"owner":"owner","body":"Original source","custom":"Keep this"},"links":links,"reason":"Capture input"})).await?;
    }
    let domain = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            address: Some("domain:d"),
            ..Default::default()
        },
    )
    .await?;
    store::execute_remote_input(pool,ws,"agent","domain-link",&json!({"op":"save_object","kind":"domain","id":"d","revision":domain["current_revision"],"parent":null,"data":domain["document"]["data"],"links":[{"target":"note:b","relation":"related"}],"reason":"Link the input"})).await?;
    store::execute_remote_input(pool,ws,"owner","process-a",&json!({"op":"decide_note","target":"note:a","revision":1,"outcome":"rejected","reason":"Input reviewed"})).await?;
    let incoming = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            kind: Some("note"),
            parent: Some("*"),
            note_status: Some("unprocessed"),
            limit: 1,
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(incoming["items"][0]["address"], "note:b");
    assert_eq!(incoming["items"][0]["note_status"], "unprocessed");
    let cursor = incoming["next_cursor"].as_str().unwrap();
    let next = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            kind: Some("note"),
            parent: Some("*"),
            note_status: Some("unprocessed"),
            limit: 1,
            cursor: Some(cursor),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(next["items"][0]["address"], "note:c");
    let related = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            kind: Some("note"),
            related_to: Some("domain:d"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        related["items"].as_array().unwrap().len(),
        3,
        "both link directions and parent membership appear"
    );
    assert_eq!(related["items"][0]["note_status"], "unprocessed");
    assert_eq!(related["items"][2]["address"], "note:a");
    assert_eq!(related["items"][2]["status"], "processed");
    let note = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            address: Some("note:a"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(note["document"]["data"]["body"], "Original source");
    assert_eq!(note["document"]["data"]["custom"], "Keep this");
    assert_eq!(note["document"]["links"].as_array().unwrap().len(), 1);
    let processed = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            kind: Some("note"),
            parent: Some("*"),
            note_status: Some("processed"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(processed["items"].as_array().unwrap().len(), 1);
    let mut data = note["document"]["data"].clone();
    data["body"] = json!("Changed input");
    store::execute_remote_input(pool,ws,"agent","edit-a",&json!({"op":"save_object","kind":"note","id":"a","revision":2,"parent":null,"data":data,"links":note["document"]["links"],"reason":"Add source information"})).await?;
    let incoming = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            kind: Some("note"),
            parent: Some("*"),
            note_status: Some("unprocessed"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(incoming["items"].as_array().unwrap().len(), 3);
    assert!(
        store::read_objects(
            pool,
            ws,
            "outsider",
            &ObjectScope {
                kind: Some("note"),
                parent: Some("*"),
                related_to: Some("domain:d"),
                ..Default::default()
            }
        )
        .await
        .is_err()
    );
    let note = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            address: Some("note:a"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(note["document"]["data"]["status"], "unprocessed");
    let found =
        store::search::find(pool, "agent", ws, "Changed input", Some("note"), 10, None).await?;
    assert_eq!(found["items"].as_array().unwrap().len(), 1);
    store::execute_remote_input(pool,ws,"agent","archive-a",&json!({"op":"archive_object","target":"note:a","revision":3,"reason":"Keep the review in history"})).await?;
    let archived = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            address: Some("note:a"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(archived["document"]["present"], false);
    assert_eq!(archived["revision_count"], 4);
    let found =
        store::search::find(pool, "agent", ws, "Changed input", Some("note"), 10, None).await?;
    assert!(found["items"].as_array().unwrap().is_empty());
    let listed = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            kind: Some("note"),
            parent: Some("*"),
            ..Default::default()
        },
    )
    .await?;
    assert!(
        !listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["address"] == "note:a")
    );
    let domain = store::read_objects(
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
        domain["link_groups"].is_array()
            && !group["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["address"] == "note:a")
    );
    let historical = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            address: Some("note:a"),
            revision: Some(1),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(historical["document"]["data"]["body"], "Original source");
    store::execute_remote_input(pool,ws,"owner","restore-a",&json!({"op":"restore_object","target":"note:a","revision":4,"reason":"Restore the source"})).await?;
    let restored = store::read_objects(
        pool,
        ws,
        "agent",
        &ObjectScope {
            address: Some("note:a"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(restored["document"]["present"], true);
    assert_eq!(restored["document"]["data"]["body"], "Changed input");
    Ok(())
}
