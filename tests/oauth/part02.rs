// Part 2 of tests/oauth.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn remote_oauth_pkce_isolation_rotation_restart_and_revocation() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(db.starts_with("joppa_test"), "isolated database required");
    store::migrate(&pool).await?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let owner = format!("oauth-owner-{suffix}");
    let ws = format!("oauth-{suffix}");
    let other = format!("other-{suffix}");
    let make: Operation = serde_json::from_value(json!({"op":"workspace","title":"OAuth test"}))?;
    store::execute(&pool, &ws, &owner, "create", &make).await?;
    store::execute(&pool, &other, "other-owner", "create", &make).await?;
    let dir = std::env::temp_dir().join(format!("joppa-oauth-{suffix}"));
    std::fs::create_dir_all(&dir)?;
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({"existing-private-service-credential":owner}).to_string(),
    )?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let base = format!("http://127.0.0.1:{port}");
    let resource = format!("{base}/mcp");
    let server = start(&url, &tokens, port).await?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let probe = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}});
    for method in [reqwest::Method::POST, reqwest::Method::GET] {
        let r = http.request(method, &resource).json(&probe).send().await?;
        assert_eq!(r.status(), 401);
        assert!(
            r.headers()["www-authenticate"]
                .to_str()?
                .contains("/.well-known/oauth-protected-resource/mcp")
        );
        assert!(
            r.headers()["www-authenticate"]
                .to_str()?
                .contains("codex mcp login joppa")
        );
        let body: Value = r.json().await?;
        assert!(
            body["error_description"]
                .as_str()
                .unwrap()
                .contains("codex mcp login joppa")
        );
    }
    let meta: Value = http
        .get(format!("{base}/.well-known/oauth-protected-resource/mcp"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(meta["resource"], resource);
    let meta: Value = http
        .get(format!("{base}/.well-known/oauth-authorization-server"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(meta["code_challenge_methods_supported"], json!(["S256"]));
    let invalid = http
        .post(format!("{base}/oauth/register"))
        .json(&json!({"redirect_uris":["http://localhost.attacker.example/callback"]}))
        .send()
        .await?;
    assert_eq!(invalid.status(), 400);
    let redirect = "http://127.0.0.1:32123/callback";
    let client:Value=http.post(format!("{base}/oauth/register")).json(&json!({"client_name":"Integration client","redirect_uris":[redirect],"token_endpoint_auth_method":"none"})).send().await?.json().await?;
    let id = client["client_id"].as_str().unwrap();
    let verifier = "test-pkce-verifier-abcdefghijklmnopqrstuvwxyz0123456789";
    let a = json!({"client_id":id,"redirect_uri":redirect,"response_type":"code","code_challenge":URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),"code_challenge_method":"S256","state":"opaque+client&state=/?","scope":"joppa","resource":resource});
    let browser = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&pool, &browser, &owner, None).await?;
    let cookie = format!("joppa_session={browser}");
    assert_eq!(
        http.get(format!("{base}/oauth/authorize?{}", form(&a)))
            .send()
            .await?
            .status(),
        200
    );
    let mut bad = a.clone();
    bad["redirect_uri"] = json!("https://attacker.example/callback");
    assert_eq!(
        http.get(format!("{base}/oauth/authorize?{}", form(&bad)))
            .send()
            .await?
            .status(),
        400
    );
    bad = a.clone();
    bad["resource"] = json!("https://attacker.example/mcp");
    assert_eq!(
        http.get(format!("{base}/oauth/authorize?{}", form(&bad)))
            .send()
            .await?
            .status(),
        400
    );
    for (origin, auth) in [(base.as_str(), false), ("https://attacker.example", true)] {
        let mut request = http
            .post(format!("{base}/oauth/approve"))
            .header("Origin", origin)
            .json(&a);
        if auth {
            request = request.header("Cookie", &cookie);
        }
        assert!(!request.send().await?.status().is_success());
    }
    assert_eq!(
        http.post(format!("{base}/oauth/approve"))
            .header("Origin", &base)
            .bearer_auth("existing-private-service-credential")
            .json(&a)
            .send()
            .await?
            .status(),
        400
    );
    let accepted: Value = http
        .post(format!("{base}/oauth/approve"))
        .header("Origin", &base)
        .header("Cookie", &cookie)
        .json(&a)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(accepted["copy_link"], true);
    assert_eq!(accepted["expires_in"], 300);
    let callback = reqwest::Url::parse(accepted["redirect"].as_str().unwrap())?;
    let pairs: std::collections::BTreeMap<_, _> = callback.query_pairs().into_owned().collect();
    assert_eq!(pairs["state"], "opaque+client&state=/?");
    let grant = json!({"grant_type":"authorization_code","client_id":id,"code":pairs["code"],"redirect_uri":redirect,"code_verifier":verifier,"resource":resource});
    for (field, value) in [
        (
            "code_verifier",
            "wrong-verifier-abcdefghijklmnopqrstuvwxyz0123456789",
        ),
        ("redirect_uri", "http://127.0.0.1:32123/other"),
        ("client_id", "other-client"),
        ("resource", "https://attacker.example/mcp"),
    ] {
        let mut bad = grant.clone();
        bad[field] = json!(value);
        assert_eq!(exchange(&http, &base, &bad).await?.status(), 400);
    }
    let r = exchange(&http, &base, &grant).await?;
    assert_eq!(r.status(), 200);
    let issued: Value = r.json().await?;
    let access = issued["access_token"].as_str().unwrap();
    let refresh = issued["refresh_token"].as_str().unwrap();
    let _server = rotation_restart_and_revocation(
        &url, &ws, &other, &tokens, port, &base, &resource, server, &http, &probe, id, &cookie,
        &grant, access, refresh,
    )
    .await?;
    fresh_consent_logout_and_manual_codes(
        &pool, &dir, &base, &resource, &http, &probe, id, verifier, &a,
        &cookie, &grant,
    )
    .await
}
