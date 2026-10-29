// Part 2 of tests/sessions.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn cookie_survives_server_restart_but_not_logout_or_expiry() -> Result<()> {
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
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&pool, &token, "owner", None).await?;
    let other = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&pool, &other, "other-owner", None).await?;
    let digest = joppa::digest_hex::sha256_hex(token.as_bytes());
    let row = pool
        .get()
        .await?
        .query_one(
            "SELECT token_hash FROM joppa.browser_sessions WHERE token_hash=$1",
            &[&digest],
        )
        .await?;
    assert_ne!(row.get::<_, String>(0), token);
    let tokens =
        std::env::temp_dir().join(format!("joppa-session-test-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(
        &tokens,
        r#"{"integration-test-key-at-least-24-chars":"owner"}"#,
    )?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();
    let cookie = format!("joppa_session={token}");
    let server = start(&url, &tokens, port).await?;
    for _ in 0..2 {
        let response = client
            .get(format!("{base}/api/me"))
            .header("cookie", &cookie)
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.json::<serde_json::Value>().await?["actor"],
            "owner"
        );
    }
    drop(server);
    let server = start(&url, &tokens, port).await?;
    let response = client
        .get(format!("{base}/api/me"))
        .header("cookie", &cookie)
        .send()
        .await?;
    assert_eq!(
        response.status(),
        200,
        "same cookie must survive a real process restart"
    );
    let response = client
        .get(format!("{base}/api/me"))
        .header("cookie", &cookie)
        .bearer_auth("invalid")
        .send()
        .await?;
    assert_eq!(
        response.status(),
        401,
        "invalid bearer must not fall back to the cookie"
    );
    let response = client
        .post(format!("{base}/api/workspaces/test/commands"))
        .header("cookie", &cookie)
        .header("origin", &base)
        .json(&json!({"request_id":"forbidden","command":{"op":"workspace","title":"Forbidden"}}))
        .send()
        .await?;
    assert_eq!(
        response.status(),
        403,
        "real authenticated browser remains read-only"
    );
    // Even a real owner's session cannot mutate notes, descriptions, attachments or order.
    let workspace = format!("readonly-{}", uuid::Uuid::new_v4());
    for (i, value) in [
        json!({"op":"workspace","title":"Read-only test"}),
        json!({"op":"save_object","kind":"domain","id":"a","revision":0,"parent":null,"data":{"title":"A","owner":"owner"},"reason":"Setup"}),
        json!({"op":"save_object","kind":"domain","id":"b","revision":0,"parent":null,"data":{"title":"B","owner":"owner"},"reason":"Setup"}),
        json!({"op":"note","target":"domain:a","id":"existing","text":"Visible comment"}),
    ]
    .into_iter()
    .enumerate()
    {
        store::execute(
            &pool,
            &workspace,
            "owner",
            &format!("setup-{i}"),
            &serde_json::from_value(value)?,
        )
        .await?;
    }
    let before = store::read(&pool, &workspace, "owner", &store::Scope::full()).await?;
    let response = client
        .get(format!("{base}/api/workspaces/{workspace}"))
        .header("cookie", &cookie)
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    for command in [
        json!({"op":"note","target":"domain:a","id":"new","text":"Forbidden"}),
        json!({"op":"describe","target":"domain:a","text":"Forbidden","reason":"edit"}),
        json!({"op":"withdraw_note","target":"domain:a","id":"existing","reason":"remove"}),
        json!({"op":"attach","target":"domain:a","id":"f","name":"a.txt","content_type":"text/plain","size":1,"hash":"a".repeat(64)}),
        json!({"op":"detach","target":"domain:a","id":"f","reason":"remove"}),
    ] {
        let response = client
            .post(format!("{base}/api/workspaces/{workspace}/commands"))
            .header("cookie", &cookie)
            .header("origin", &base)
            .json(&json!({"request_id":"forbidden", "command":command}))
            .send()
            .await?;
        assert_eq!(response.status(), 403);
        assert!(response.text().await?.contains("read-only"));
    }
    assert_eq!(
        client
            .post(format!("{base}/api/workspaces/{workspace}/blobs"))
            .header("cookie", &cookie)
            .header("origin", &base)
            .body("Forbidden upload")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(client.post(format!("{base}/api/workspaces/{workspace}/order"))
        .header("cookie", &cookie).header("origin", &base)
        .json(&json!({"request_id":"move","kind":"domain","parent":null,"expected":["a","b"],"ids":["b","a"]}))
        .send().await?.status(), 403);
    assert_eq!(
        store::read(&pool, &workspace, "owner", &store::Scope::full()).await?,
        before
    );
    let response = client
        .post(format!("{base}/auth/logout"))
        .header("cookie", &cookie)
        .send()
        .await?;
    assert_eq!(response.status(), 403, "logout requires same origin");
    assert!(sessions::actor(&pool, &token).await?.is_some());
    let response = client
        .post(format!("{base}/auth/logout"))
        .header("cookie", &cookie)
        .header("origin", &base)
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    assert!(
        response.headers()["set-cookie"]
            .to_str()?
            .contains("Max-Age=0")
    );
    drop(server);
    let _server = start(&url, &tokens, port).await?;
    let response = client
        .get(format!("{base}/api/me"))
        .header("cookie", &cookie)
        .send()
        .await?;
    assert_eq!(
        response.status(),
        401,
        "revocation must also survive restart"
    );
    assert_eq!(
        sessions::actor(&pool, &other).await?,
        Some(("other-owner".into(), None))
    );
    sessions::revoke(&pool, &token).await?; // Repeated logout is harmless.
    let expired = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    pool.get().await?.execute("INSERT INTO joppa.browser_sessions(token_hash,actor,expires_at) VALUES($1,'owner',clock_timestamp()-interval '1 second')",&[&joppa::digest_hex::sha256_hex(expired.as_bytes())]).await?;
    assert!(sessions::actor(&pool, &expired).await?.is_none());
    assert!(sessions::actor(&pool, "malformed").await?.is_none());
    let response = client
        .get(format!("{base}/api/me"))
        .header("cookie", format!("joppa_session={expired}"))
        .send()
        .await?;
    assert_eq!(response.status(), 401);
    std::fs::remove_file(tokens)?;
    Ok(())
}
