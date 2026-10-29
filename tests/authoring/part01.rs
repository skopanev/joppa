// Part 1 of tests/authoring.rs, included there, so the test names stay unchanged.

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
        &json!({"op":"enroll_service","id":"agent","role":"agent"}),
    )
    .await?;
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "consumer",
        &json!({"op":"save_object","kind":"consumer","id":"buyer","revision":0,"parent":null,"data":{"title":"Buyer","owner":"owner"},"reason":"Setup"}),
    )
    .await?;
    let input = json!({"op":"save_object","kind":"domain","revision":0,"parent":null,"data":{"title":"Area","owner":"owner"},"reason":"Create"});
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
    // Item IDs start with the prefix of the workspace, a slug of its title.
    let prefix: String = pool.get().await?.query_one("SELECT COALESCE(state->>'reference_prefix',workspace) FROM joppa.board_cache WHERE workspace=$1", &[&ws]).await?.get(0);
    assert!(public_id.starts_with(&format!("{prefix}-D-")));
    assert_eq!(
        resolve_id::resolve(&pool, "owner", &ws, public_id).await?,
        format!("domain:{id}")
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
            &json!({"op":"save_object","kind":"domain","revision":0,"parent":null,"data":{"title":"Changed","owner":"owner"},"reason":"Create"})
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
        {"ref":"area","command":{"op":"save_object","kind":"domain","revision":0,"parent":null,"data":{"title":"Second area","owner":"owner"},"reason":"Agreed scope"}},
        {"ref":"ability","command":{"op":"save_object","kind":"capability","revision":0,"parent":{"$ref":"area/address"},"data":{"title":"Keep paid access","owner":"owner"},"reason":"Agreed scope"}},
        {"ref":"requirement","command":{"op":"save_object","kind":"requirement","revision":0,"parent":{"$ref":"ability/address"},"data":{"title":"Paid access persists","body":"Only iOS","owner":"owner"},"reason":"Agreed scope"}},
        {"ref":"criterion","command":{"op":"save_object","kind":"ac","revision":0,"parent":{"$ref":"requirement/address"},"data":{"title":"On iOS access survives sign-in","owner":"owner","consumer":"buyer","before_launch":false,"decision_required":false,"depends_on":[]},"reason":"Agreed scope"}},
        {"command":{"op":"comment","req":{"$ref":"requirement/address"},"revision":2,"text":"Please review","kind":"comment","ac":{"$ref":"criterion/address"}}}
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
        a["results"][1]["command"]["parent"],
        format!(
            "domain:{}",
            a["results"][0]["command"]["id"].as_str().unwrap()
        )
    );
    let req = a["results"][2]["command"]["id"].as_str().unwrap();
    let ac = a["results"][3]["command"]["id"].as_str().unwrap();
    let domain = a["results"][0]["command"]["id"].as_str().unwrap();
    let capability = a["results"][1]["command"]["id"].as_str().unwrap();
    let first = store::board_read(&pool, &ws, "owner").await?;
    assert!(first["state"].get("domains").is_none());
    assert!(first.get("history").is_none());
    assert!(first.get("system_ids").is_none());
    let domain_key = format!("domain:{domain}");
    let branch = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            parent: Some(&domain_key),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(branch["items"][0]["id"], capability);
    let capability_key = format!("capability:{capability}");
    let branch = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            parent: Some(&capability_key),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(branch["items"][0]["title"], "Paid access persists");
    assert!(branch["items"][0].get("document").is_none());
    assert!(branch["items"][0].get("acs").is_none());
    let consumers = store::read_objects(
        &pool,
        &ws,
        "owner",
        &store::ObjectScope {
            kind: Some("consumer"),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(consumers["items"][0]["title"], "Buyer");
    uuid::Uuid::parse_str(req)?;
    uuid::Uuid::parse_str(ac)?;
    assert!(
        a["results"][3]["system_ids"]["ac"][ac]
            .as_str()
            .unwrap()
            .starts_with(&format!("{prefix}-A-"))
    );
    let before = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    // Each failed package leaves neither a partial tree nor a consumed request ID.
    for bad in [
        json!({"op":"save_object","kind":"capability","revision":0,"parent":"domain:absent","data":{"title":"Broken","owner":"owner"},"reason":"Broken"}),
        json!({"op":"confirm_tree"}),
        json!({"op":"enroll_member","id":"intruder"}),
        json!({"op":"save_object","kind":"capability","revision":0,"parent":{"$ref":"future/address"},"data":{"title":"Broken","owner":"owner"},"reason":"Broken"}),
        json!({"op":"save_object","kind":"domain","revision":0,"parent":null,"data":{"title":"Broken","owner":"owner"},"reason":"Broken","actor":"owner"}),
        // A removed operation makes the whole package fail.
        json!({"op":"domain","title":"Broken","authority":"owner"}),
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
        assert_eq!(
            store::read(&pool, &ws, "owner", &store::Scope::full()).await?,
            before
        );
    }
    store::execute_batch(&pool, &ws, "agent", "retry-after-error", &operations).await?;
    let mut revision = a["results"][2]["command"].clone();
    revision["revision"] = json!(2);
    revision["data"]["title"] = json!("Access persists after signing in again");
    let updated = store::execute_input(&pool, &ws, "agent", "revise", &revision).await?;
    assert_eq!(updated["command"]["id"], req);
    assert_eq!(updated["system_ids"], a["results"][2]["system_ids"]);
    revision.as_object_mut().unwrap().remove("kind");
    assert!(
        store::execute_input(&pool, &ws, "agent", "missing-req", &revision)
            .await
            .is_err()
    );
    // Generated values persist as journal facts, and receipts cannot be rewritten.
    let after = store::read(&pool, &ws, "owner", &store::Scope::full()).await?;
    store::migrate(&pool).await?;
    assert_eq!(
        store::read(&pool, &ws, "owner", &store::Scope::full()).await?,
        after
    );
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
    conn.execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&ws])
        .await?;
    assert!(store::backfill_board(&pool).await? >= 1);
    let domain_key = format!("domain:{domain}");
    assert_eq!(
        store::read_objects(
            &pool,
            &ws,
            "owner",
            &store::ObjectScope {
                address: Some(&domain_key),
                ..Default::default()
            }
        )
        .await?["document"]["data"]["title"],
        "Second area"
    );
    Ok(())
}
