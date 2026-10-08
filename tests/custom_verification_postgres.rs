use deadpool_postgres::Pool;
use joppa::store::{self, ObjectScope};
use serde_json::{Value, json};
async fn apply(pool: &Pool, ws: &str, actor: &str, command: Value) -> anyhow::Result<Value> {
    store::execute_remote_input(pool, ws, actor, &uuid::Uuid::new_v4().to_string(), &command).await
}
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn custom_verification_projection_preserves_addresses_history_and_nearest_context()
-> anyhow::Result<()> {
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
    let ws = format!("generic-checks-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "create",
        &json!({"op":"workspace","title":"Generic verification"}),
    )
    .await?;
    for (id, role) in [("agent", "agent"), ("runner", "runner")] {
        apply(
            &pool,
            &ws,
            "owner",
            json!({"op":"member","id":id,"role":role}),
        )
        .await?;
    }
    let schema = store::tree_schema(&pool, &ws, "agent", None).await?;
    let types = json!({
        "spec":{"title":"Spec","role":"requirement","root":true,"parents":["*"]},
        "epic":{"title":"Epic","role":"requirement","root":true,"parents":["*"]},
        "section":{"title":"Section","role":"group","root":true,"parents":["*"]},
        "measure":{"title":"Measure","role":"ac","root":false,"parents":["*"]},
        "rule":{"title":"Rule","role":"ac","root":false,"parents":["*"]},
        "work":{"title":"Work","role":"task","root":false,"parents":["*"],"parent_gate":"ready"}
    });
    apply(&pool,&ws,"agent",json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Use configured type names"})).await?;
    for (kind, id, parent, data) in [
        (
            "spec",
            "same",
            None,
            json!({"title":"Outer","body":"Outer spec","owner":"owner"}),
        ),
        (
            "epic",
            "same",
            Some("spec:same"),
            json!({"title":"Inner","body":"Inner spec","owner":"owner","rationale":"The input source"}),
        ),
        (
            "section",
            "middle",
            Some("epic:same"),
            json!({"title":"Middle","owner":"owner"}),
        ),
        (
            "measure",
            "same",
            Some("section:middle"),
            json!({"title":"Measure","owner":"owner"}),
        ),
        (
            "rule",
            "same",
            Some("section:middle"),
            json!({"title":"Rule","owner":"owner"}),
        ),
    ] {
        apply(&pool,&ws,"agent",json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"reason":"Define the specification"})).await?;
    }
    for (kind, id, query) in [
        ("spec", "same", "Outer"),
        ("epic", "same", "Inner"),
        ("measure", "same", "Measure"),
    ] {
        let found = store::search::find(&pool, "agent", &ws, query, Some(kind), 10, None).await?;
        assert_eq!(found["items"][0]["kind"], kind);
        assert_eq!(found["items"][0]["id"], id);
    }
    let process = store::board_read(
        &pool,
        &ws,
        "agent",
        Some("process"),
        Some("measure:same"),
        None,
    )
    .await?;
    assert_eq!(process["req"], "epic:same");
    assert_eq!(process["revision"], 3);
    let ids: Vec<&str> = process["criteria"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["measure:same", "rule:same"]);
    assert!(
        store::board_read(
            &pool,
            &ws,
            "outsider",
            Some("process"),
            Some("measure:same"),
            None
        )
        .await
        .is_err()
    );
    apply(&pool,&ws,"agent",json!({"op":"save_object","kind":"section","id":"unbound","revision":0,"parent":null,"data":{"title":"No context","owner":"owner"},"reason":"Create an independent group"})).await?;
    assert!(
        store::board_read(
            &pool,
            &ws,
            "agent",
            Some("process"),
            Some("section:unbound"),
            None
        )
        .await
        .is_err()
    );
    apply(
        &pool,
        &ws,
        "owner",
        json!({"op":"confirm","req":"epic:same","revision":3}),
    )
    .await?;
    apply(&pool,&ws,"owner",json!({"op":"define_verification_scope","req":"epic:same","revision":3,"subject_keys":["build"],"scope":"The exact custom build","reason":"Agree on the subject"})).await?;
    for ac in ["measure:same", "rule:same"] {
        apply(&pool,&ws,"agent",json!({"op":"assess","req":"epic:same","revision":3,"ac":ac,"assessment":{"ready":true,"reason":"The contract is clear"}})).await?;
    }
    apply(&pool,&ws,"agent",json!({"op":"save_object","kind":"work","id":"one","revision":0,"parent":"measure:same","data":{"title":"Implement","owner":"owner","contract":"Implement the criterion","purpose":"implementation"},"reason":"Create work"})).await?;
    apply(
        &pool,
        &ws,
        "agent",
        json!({"op":"claim_task","id":"work:one","revision":1}),
    )
    .await?;
    let checks:Vec<Value>=["measure:same","rule:same"].iter().map(|ac|json!({"id":format!("[{ac}] real check"),"ac":ac,"method":"automated","expected":["observed behavior"]})).collect();
    apply(&pool,&ws,"runner",json!({"op":"plan","req":"epic:same","revision":3,"run":"current","subject":{"build":"sha-one"},"scope":"The exact custom build","discovery_complete":true,"checks":checks})).await?;
    for ac in ["measure:same", "rule:same"] {
        apply(&pool,&ws,"runner",json!({"op":"result","req":"epic:same","revision":3,"run":"current","evidence":{"check":format!("[{ac}] real check"),"outcome":"pass","observed":["observed behavior"],"note":"Executed the exact check"}})).await?;
    }
    apply(
        &pool,
        &ws,
        "runner",
        json!({"op":"finish","req":"epic:same","revision":3,"run":"current"}),
    )
    .await?;
    apply(
        &pool,
        &ws,
        "owner",
        json!({"op":"accept","req":"epic:same","revision":3,"run":"current"}),
    )
    .await?;
    apply(
        &pool,
        &ws,
        "agent",
        json!({"op":"note","target":"measure:same","id":"source","text":"Evidence context"}),
    )
    .await?;
    let detail = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            address: Some("measure:same"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(detail["process"]["verification"], "pass");
    assert_eq!(
        detail["verification_summary"],
        json!({"total":1,"passed":1})
    );
    for address in ["spec:same", "epic:same", "section:middle"] {
        let parent = store::read_objects(
            &pool,
            &ws,
            "agent",
            &ObjectScope {
                address: Some(address),
                ..Default::default()
            },
        )
        .await?;
        assert_eq!(
            parent["verification_summary"],
            json!({"total":2,"passed":2})
        );
    }
    let historical = store::read_objects(
        &pool,
        &ws,
        "agent",
        &ObjectScope {
            address: Some("epic:same"),
            revision: Some(1),
            ..Default::default()
        },
    )
    .await?;
    assert!(historical.get("verification_summary").is_none());
    assert_eq!(
        detail["annotations"]["comments"]["source"]["text"],
        "Evidence context"
    );
    let process =
        store::board_read(&pool, &ws, "agent", Some("process"), Some("work:one"), None).await?;
    assert_eq!(process["req"], "epic:same");
    assert_eq!(
        process["result_dates"]["current"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(process["summary"]["accepted"], true);
    let before:Vec<(String,i32,String)>=pool.get().await?.query("SELECT address,revision,md5(document::text) FROM joppa.object_revisions WHERE workspace=$1 ORDER BY address,revision",&[&ws]).await?.iter().map(|r|(r.get(0),r.get(1),r.get(2))).collect();
    pool.get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    store::migrate(&pool).await?;
    let after:Vec<(String,i32,String)>=pool.get().await?.query("SELECT address,revision,md5(document::text) FROM joppa.object_revisions WHERE workspace=$1 ORDER BY address,revision",&[&ws]).await?.iter().map(|r|(r.get(0),r.get(1),r.get(2))).collect();
    assert_eq!(before, after);
    apply(&pool,&ws,"agent",json!({"op":"note","target":"epic:same","id":"after-replay","text":"The history remains valid"})).await?;
    let read = store::read(
        &pool,
        &ws,
        "agent",
        &store::Scope::from_args(&json!({"req":"epic:same"}))?,
    )
    .await?;
    assert_eq!(read["summary"]["accepted"], true);
    Ok(())
}
