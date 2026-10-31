// Part 2 of tests/claims_postgres.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn one_agent_claims_two_tasks_in_parallel_and_a_third_over_http() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        db.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let ws = format!("single-claim-{}", uuid::Uuid::new_v4().simple());
    for (id, command) in [
        (
            "workspace",
            json!({"op":"workspace","title":"Several active tasks per agent"}),
        ),
        (
            "bot",
            json!({"op":"enroll_service","id":"bot","role":"agent"}),
        ),
        (
            "bot2",
            json!({"op":"enroll_service","id":"bot2","role":"agent"}),
        ),
    ] {
        store::execute_input(&pool, &ws, "owner", id, &command).await?;
    }
    let current = store::tree_schema(&pool, &ws, "owner", None).await?;
    let types = json!({
        "spec":{"title":"Specs","role":"document","status":"none","root":true,"parents":["*"]},
        "job":{"title":"Jobs","role":"task","status":"none","root":true,"parents":["*"]},
        "work":{"title":"Work","role":"task","status":"none","root":true,"parents":["*"]}
    });
    store::execute_remote_input(&pool, &ws, "owner", "schema", &json!({"op":"set_tree_schema","revision":current["schema"]["revision"],"types":types,"reason":"Use two configured work types"})).await?;
    for (kind, id) in [("job", "one"), ("work", "one"), ("job", "two")] {
        store::execute_remote_input(&pool, &ws, "owner", &format!("{kind}-{id}"), &json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":null,"data":{"title":"Implement the contract","owner":"owner","body":"Implement and test the specified behavior","projects":[]},"reason":"Create work"})).await?;
    }
    let claim_a = json!({"op":"claim_task","id":"job:one","revision":1});
    let claim_b = json!({"op":"claim_task","id":"work:one","revision":1});
    let (a, b) = tokio::join!(
        store::execute_remote_input(&pool, &ws, "bot", "claim-a", &claim_a),
        store::execute_remote_input(&pool, &ws, "bot", "claim-b", &claim_b)
    );
    ensure!(a.is_ok() && b.is_ok(), "both claims succeed: {a:?} / {b:?}");
    let mine = store::task_queue(&pool, &ws, "bot", "mine", 100, None).await?;
    ensure!(mine["tasks"].as_array().unwrap().len() == 2, "{mine}");
    let available = store::task_queue(&pool, &ws, "bot", "available", 100, None).await?;
    ensure!(
        available["tasks"].as_array().unwrap().len() == 1
            && available["tasks"][0]["address"] == "job:two"
            && available["tasks"][0]["claimable"] == true,
        "{available}"
    );
    let dir = std::env::temp_dir().join(format!(
        "joppa-claim-http-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir)?;
    let token = uuid::Uuid::new_v4().to_string();
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, json!({token.clone():"bot"}).to_string())?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let _server = Server {
        child: std::process::Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env(
                "JOPPA_DATABASE_URL",
                std::env::var("JOPPA_TEST_DATABASE_URL")?,
            )
            .env("JOPPA_TOKENS_FILE", &tokens)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .env("DEEPINFRA_API_KEY", "test-only-key")
            .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
            .envs(fake_spaces::start().env())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?,
        dir,
    };
    for _ in 0..100 {
        if reqwest::get(format!("{root}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let response = reqwest::Client::new().post(format!("{root}/api/workspaces/{ws}/commands"))
        .bearer_auth(&token)
        .json(&json!({"request_id":"http-third-claim","command":{"op":"claim_task","id":"job:two","revision":1}}))
        .send().await?;
    ensure!(response.status().is_success(), "{}", response.status());
    let mine = store::task_queue(&pool, &ws, "bot", "mine", 100, None).await?;
    ensure!(mine["tasks"].as_array().unwrap().len() == 3, "{mine}");
    let before = store::read_objects(
        &pool,
        &ws,
        "bot",
        &store::ObjectScope {
            address: Some("job:two"),
            ..Default::default()
        },
    )
    .await?;
    let next = store::execute_remote_input(
        &pool,
        &ws,
        "bot",
        "next-while-held",
        &json!({"op":"claim_next_task"}),
    )
    .await
    .unwrap_err();
    ensure!(
        next.to_string() == "no available Task matches this claim",
        "{next:#}"
    );
    let after = store::read_objects(
        &pool,
        &ws,
        "bot",
        &store::ObjectScope {
            address: Some("job:two"),
            ..Default::default()
        },
    )
    .await?;
    ensure!(
        before["position"] == after["position"],
        "a refused next must not write an event"
    );
    Ok(())
}
