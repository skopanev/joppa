// Part 2 of tests/create_workspace.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn browser_user_creates_and_owns_a_workspace() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
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
    let id = uuid::Uuid::new_v4().simple().to_string();
    let (alex, sam) = (format!("alex-{id}"), format!("sam-{id}"));
    let (base, hidden) = (format!("base-{id}"), format!("hidden-{id}"));
    for workspace in [&base, &hidden] {
        store::execute(
            &pool,
            workspace,
            "owner",
            "create",
            &Command::Workspace {
                title: workspace.clone(),
            },
        )
        .await?;
        for (i, member) in [&alex, &sam].into_iter().enumerate() {
            store::execute(
                &pool,
                workspace,
                "owner",
                &format!("member-{i}"),
                &Command::Member {
                    id: member.clone(),
                    role: "human".into(),
                },
            )
            .await?;
        }
    }
    let dir = std::env::temp_dir().join(format!("joppa-create-{id}"));
    std::fs::create_dir_all(&dir)?;
    let (tokens, rules, users) = (
        dir.join("tokens.json"),
        dir.join("access.json"),
        dir.join("users.json"),
    );
    let alex_key = format!("alex-key-{id}");
    std::fs::write(
        &tokens,
        json!({format!("owner-key-{id}"):"owner", alex_key.clone():alex, format!("sam-key-{id}"):sam})
            .to_string(),
    )?;
    // The rules name only the base workspace. Neither user has a rule for hidden.
    std::fs::write(
        &rules,
        json!({"emails":{"alex@example.test":[base],"sam@example.test":[base]}}).to_string(),
    )?;
    std::fs::write(
        &users,
        json!({"alex@example.test":alex,"sam@example.test":sam}).to_string(),
    )?;
    let (alex_cookie, sam_cookie) = (
        format!("joppa_session={}{}", id, uuid::Uuid::new_v4().simple()),
        format!("joppa_session={}{}", uuid::Uuid::new_v4().simple(), id),
    );
    sessions::create(&pool, alex_cookie.split_once('=').unwrap().1, &alex, None).await?;
    sessions::create(&pool, sam_cookie.split_once('=').unwrap().1, &sam, None).await?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let _server = Server(
        Process::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("JOPPA_DATABASE_URL", &url)
            .env("JOPPA_TOKENS_FILE", &tokens)
            .env("JOPPA_LOGIN_USERS_FILE", &users)
            .env("JOPPA_ACCESS_RULES_FILE", &rules)
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env("DEEPINFRA_API_KEY", "test-only-key")
            .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
            .envs(fake_spaces::start().env())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
        dir,
    );
    let root = format!("http://127.0.0.1:{port}");
    let mut ready = false;
    for _ in 0..100 {
        if reqwest::get(format!("{root}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    ensure!(ready, "test server did not start");
    let client = reqwest::Client::new();
    let get = |path: String, cookie: &str| browser_get(&client, &root, path, cookie);
    let create = |body: Value, cookie: Option<&str>, origin: Option<&str>| {
        browser_create(&client, &root, body, cookie, origin)
    };
    let response = get("/api/workspaces".into(), &alex_cookie).await?;
    assert!(response.status().is_success());
    let list_before = response.json::<Value>().await?;
    assert_eq!(list_before, json!([{"id":base,"title":base}]));
    let unchanged = snapshot(&pool).await?;

    // Step 1: a session, write permission and the same origin are required.
    let response = create(json!({"title":"Billing"}), None, Some(&root)).await?;
    assert_eq!(response.status(), 401);
    assert_eq!(
        response.json::<Value>().await?,
        json!({"error":"Please sign in to continue"})
    );
    for origin in [Some("http://evil.example"), None] {
        let response = create(json!({"title":"Billing"}), Some(&alex_cookie), origin).await?;
        assert_eq!(response.status(), 403, "origin {origin:?}");
    }
    let response = client
        .post(format!("{root}/api/workspaces"))
        .bearer_auth(&alex_key)
        .header("origin", &root)
        .json(&json!({"title":"Billing"}))
        .send()
        .await?;
    assert_eq!(
        response.status(),
        403,
        "a bearer key is not a browser session"
    );
    // Step 2: the client cannot choose either ID.
    for body in [
        json!({"title":"Billing","id":"chosen"}),
        json!({"title":"Billing","request_id":"chosen"}),
    ] {
        let response = create(body, Some(&alex_cookie), Some(&root)).await?;
        assert!(response.status().is_client_error());
    }
    // Step 4: the Workspace command validates the title and the transaction rolls back.
    let response = create(json!({"title":"  "}), Some(&alex_cookie), Some(&root)).await?;
    assert!(response.status().is_client_error());
    assert_eq!(
        response.json::<Value>().await?,
        json!({"error":"text must not be empty"})
    );
    assert_eq!(snapshot(&pool).await?, unchanged);
    let response = get("/api/workspaces".into(), &alex_cookie).await?;
    assert!(response.status().is_success());
    assert_eq!(response.json::<Value>().await?, list_before);

    billing_is_created_owned_and_refused_as_specified(&pool, &id, &alex, &base, &hidden, &alex_key, &alex_cookie, &sam_cookie, &root, &client).await
}
