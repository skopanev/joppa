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
            json!({"op":"member","id":"agent","role":"agent"}),
        ),
        (
            "other",
            "owner",
            json!({"op":"member","id":"other","role":"agent"}),
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
                index: None,
                references: Default::default(),
                parent_decision: false,
                inbox: None,
                launch_blocker: None,
                color: None,
                title: kind.into(),
                role: role.into(),
                root,
                parents: parents.into_iter().map(String::from).collect(),
                parent_gate: None,
            },
        );
    }
    store::execute_remote_input(&pool,&ws,"agent","schema",&json!({"op":"set_tree_schema","revision":current["schema"]["revision"],"types":types,"reason":"Use specifications without REQ or AC"})).await?;
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
    let rest = store::execute_remote_input(
        &pool,
        &ws,
        "agent",
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
    store::execute_remote_input(&pool,&ws,"agent","generic-ac-schema",&json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Permit criteria under specifications"})).await?;
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
