#[allow(dead_code)]
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
        &json!({"op":"member","id":"agent","role":"agent"}),
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
        {"ref":"requirement","command":{"op":"revise","expected_revision":0,"capability":{"$ref":"ability/id"},"title":"Paid access persists","body":"Only iOS","owner":"owner","reason":"Agreed scope","acs":[{"text":"On iOS access survives sign-in"}]}},
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
        assert_eq!(store::read(&pool, &ws, "owner", &store::Scope::full()).await?, before);
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
    assert_eq!(store::read(&pool, &ws, "owner", &store::Scope::full()).await?, after);
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
    Ok(())
}
