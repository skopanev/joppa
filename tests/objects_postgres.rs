mod common;
use common::*;
use joppa::store::{self, ObjectScope};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn objects_replay_concurrency_ids_and_schema_are_atomic() -> anyhow::Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let conn = pool.get().await?;
    let name: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    drop(conn);
    store::migrate(&pool).await?;
    let ws = format!(
        "objects-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    for (n, cmd) in setup().into_iter().enumerate() {
        store::execute_input(&pool, &ws, "owner", &format!("setup-{n}"), &cmd).await?;
    }
    // Projection writes must work with the production role's restricted grants.
    let conn = pool.get().await?;
    conn.batch_execute("DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='joppa_test_projection_guard') THEN CREATE ROLE joppa_test_projection_guard; END IF; END $$; GRANT USAGE ON SCHEMA joppa TO joppa_test_projection_guard; GRANT SELECT,INSERT ON ALL TABLES IN SCHEMA joppa TO joppa_test_projection_guard; GRANT UPDATE ON joppa.objects,joppa.board_cache,joppa.state_revisions,joppa.state_entries,joppa.search_documents,joppa.search_state,joppa.search_revisions TO joppa_test_projection_guard; GRANT DELETE ON joppa.state_entries,joppa.task_queue TO joppa_test_projection_guard; GRANT DELETE ON joppa.object_links,joppa.search_documents,joppa.search_state TO joppa_test_projection_guard; GRANT USAGE,SELECT ON ALL SEQUENCES IN SCHEMA joppa TO joppa_test_projection_guard;").await?;
    conn.batch_execute("CREATE OR REPLACE FUNCTION joppa.test_projection_no_rewrite() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'projection updated an unchanged row'; END $$;").await?;
    let statement = format!(
        "CREATE TRIGGER test_projection_no_rewrite BEFORE UPDATE ON joppa.objects FOR EACH ROW WHEN (OLD.workspace='{}' AND OLD.address='domain:d') EXECUTE FUNCTION joppa.test_projection_no_rewrite()",
        ws
    );
    conn.batch_execute(&statement).await?;
    drop(conn);
    let restricted_url = format!(
        "{}{}options=-c%20role%3Djoppa_test_projection_guard",
        std::env::var("JOPPA_TEST_DATABASE_URL")?,
        if std::env::var("JOPPA_TEST_DATABASE_URL")?.contains('?') {
            "&"
        } else {
            "?"
        }
    );
    let restricted = store::pool(&restricted_url)?;
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
        &ws,
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
        store::execute_remote_input(&restricted, &ws, "agent", "edit-a", &edit_a),
        store::execute_remote_input(&restricted, &ws, "agent", "edit-b", &edit_b)
    );
    assert_ne!(a.is_ok(), b.is_ok(), "one stale writer must lose");
    let (id, body, result) = if a.is_ok() {
        ("edit-a", "A", a?)
    } else {
        ("edit-b", "B", b?)
    };
    let retry = store::execute_remote_input(&pool, &ws, "agent", id, &make(body, 1)).await?;
    assert_eq!(retry["position"], result["position"]);
    assert_eq!(retry["replayed"], true);
    assert!(
        store::execute_remote_input(&pool, &ws, "agent", id, &make("Changed retry", 1))
            .await
            .is_err()
    );
    let old = store::read_objects(
        &pool,
        &ws,
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
        store::read_item(&pool, public, "agent").await?["address"],
        "note:source"
    );
    assert!(
        store::read_objects(&pool, &ws, "outsider", &ObjectScope::default())
            .await
            .is_err()
    );
    assert!(
        store::tree_schema(&pool, &ws, "outsider", None)
            .await
            .is_err()
    );
    let parent = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            address: Some("domain:d"),
            ..Default::default()
        },
    )
    .await?;
    let group = store::link_page(
        &pool, &ws, "agent", "domain:d", "incoming", "related", "note", 100, None,
    )
    .await?;
    assert!(
        group["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["address"] == "note:source")
    );
    let listed = store::read_objects(&pool, &ws, "agent", &ObjectScope::default()).await?;
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
    let batch = store::execute_remote_batch(&pool, &ws, "agent", "note-batch", &ops).await?;
    assert_eq!(batch["results"][1]["object"]["revision"], 2);
    let replay = store::execute_remote_batch(&pool, &ws, "agent", "note-batch", &ops).await?;
    assert_eq!(batch["results"], replay["results"]);
    store::execute_remote_input(&pool,&ws,"owner","default-schema",&json!({"op":"set_tree_schema","revision":store::tree_schema(&pool,&ws,"agent",None).await?["current_revision"],"types":joppa::objects::defaults(),"reason":"Record the default schema"})).await?;
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
        &pool,
        &ws,
        "owner",
        "schema",
        &json!({"op":"set_tree_schema","revision":store::tree_schema(&pool,&ws,"agent",None).await?["current_revision"],"types":types,"reason":"Use an initiative"}),
    )
    .await?;
    store::execute_remote_input(&pool,&ws,"agent","group",&json!({"op":"save_object","kind":"initiative","id":"release","revision":0,"parent":null,"data":{"title":"Release","owner":"owner"},"reason":"Group release work"})).await?;
    store::execute_remote_input(&pool,&ws,"agent","req",&json!({"op":"save_object","kind":"requirement","id":"custom","revision":0,"parent":"initiative:release","data":{"title":"Open the release","body":"Show the current release","owner":"owner","consumers":["customer"]},"reason":"Define the release"})).await?;
    let children = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            parent: Some("initiative:release"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(children["items"][0]["address"], "requirement:custom");
    let state = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    assert_eq!(
        state["state"]["objects"]["requirement:custom"]["revisions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let count: Value = state["state"]["objects"].clone();
    store::migrate(&pool).await?;
    assert_eq!(
        store::read(&pool, &ws, "owner", &store::Scope::full()).await?["state"]["objects"],
        count
    );
    assert_eq!(
        store::tree_schema(&pool, &ws, "agent", None).await?["current_revision"],
        // migrate_consumers adds one revision: one consumer per criterion. migrate_test_rules
        // adds one more: the test rules and the status rule of each type. migrate_note_parents
        // adds one more: an inbox type has no parent.
        9
    );
    assert_eq!(
        store::tree_schema(&pool, &ws, "agent", Some(0)).await?["schema"]["revision"],
        0
    );
    let found =
        store::search::find(&pool, "agent", &ws, "Generated", Some("note"), 10, None).await?;
    assert!(
        found["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "note")
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn note_queue_filters_links_and_color_preserve_board_structure() -> anyhow::Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    let ws = format!(
        "note-queue-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    for (n, cmd) in setup().into_iter().enumerate() {
        store::execute_input(&pool, &ws, "owner", &format!("setup-{n}"), &cmd).await?;
    }
    let schema = store::tree_schema(&pool, &ws, "agent", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["note"]["color"] = json!("#7655a2");
    types["note"]["title_size"] = json!(21);
    types["note"]["title"] = json!("Incoming note");
    types["domain"]["index"] = json!(200);
    store::execute_remote_input(&pool, &ws, "owner", "color", &json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Distinguish source Notes"})).await?;
    let board = store::board_read(&pool, &ws, "agent", None, None, None).await?;
    assert_eq!(board["custom_tree"], false);
    assert_eq!(board["object_types"]["note"]["color"], "#7655a2");
    assert_eq!(board["object_types"]["note"]["title_size"], 21);
    assert_eq!(board["object_types"]["domain"]["index"], 200);
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
        store::execute_remote_input(&pool,&ws,"agent",id,&json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":parent,"data":{"title":id,"owner":"owner","body":"Original source","custom":"Keep this"},"links":links,"reason":"Capture input"})).await?;
    }
    let domain = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            address: Some("domain:d"),
            ..Default::default()
        },
    )
    .await?;
    store::execute_remote_input(&pool,&ws,"agent","domain-link",&json!({"op":"save_object","kind":"domain","id":"d","revision":domain["current_revision"],"parent":null,"data":domain["document"]["data"],"links":[{"target":"note:b","relation":"related"}],"reason":"Link the input"})).await?;
    store::execute_remote_input(&pool,&ws,"agent","process-a",&json!({"op":"set_note_status","target":"note:a","revision":1,"status":"processed","reason":"Input reviewed"})).await?;
    let incoming = store::read_objects(
        &pool,
        &ws,
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
        &pool,
        &ws,
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
        &pool,
        &ws,
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
        &pool,
        &ws,
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
        &pool,
        &ws,
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
    store::execute_remote_input(&pool,&ws,"agent","edit-a",&json!({"op":"save_object","kind":"note","id":"a","revision":2,"parent":null,"data":data,"links":note["document"]["links"],"reason":"Add source information"})).await?;
    let incoming = store::read_objects(
        &pool,
        &ws,
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
            &pool,
            &ws,
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
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            address: Some("note:a"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(note["document"]["data"]["status"], "unprocessed");
    let found =
        store::search::find(&pool, "agent", &ws, "Changed input", Some("note"), 10, None).await?;
    assert_eq!(found["items"].as_array().unwrap().len(), 1);
    store::execute_remote_input(&pool,&ws,"agent","archive-a",&json!({"op":"archive_object","target":"note:a","revision":3,"reason":"Keep the review in history"})).await?;
    let archived = store::read_objects(
        &pool,
        &ws,
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
        store::search::find(&pool, "agent", &ws, "Changed input", Some("note"), 10, None).await?;
    assert!(found["items"].as_array().unwrap().is_empty());
    let listed = store::read_objects(
        &pool,
        &ws,
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
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            address: Some("domain:d"),
            ..Default::default()
        },
    )
    .await?;
    let group = store::link_page(
        &pool, &ws, "agent", "domain:d", "incoming", "related", "note", 100, None,
    )
    .await?;
    assert!(
        domain["link_groups"].is_array()
            && !group["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["address"] == "note:a")
    );
    let historical = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            address: Some("note:a"),
            revision: Some(1),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(historical["document"]["data"]["body"], "Original source");
    store::execute_remote_input(&pool,&ws,"owner","restore-a",&json!({"op":"restore_object","target":"note:a","revision":4,"reason":"Restore the source"})).await?;
    let restored = store::read_objects(
        &pool,
        &ws,
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
