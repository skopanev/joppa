// Email grants: a member who did not create W adds a person, an agent cannot, and a malformed
// email is refused.
// Included into web_tests.rs, so the test paths stay web::tests::<name>.
#[tokio::test]
#[ignore = "requires isolated test database"]
async fn a_member_who_did_not_create_w_adds_maria_and_the_agent_cannot_add_lena() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(db.starts_with("joppa_test"), "test database required");
    store::migrate(&pool).await?;
    let app = App {
        pool: pool.clone(),
        identities: Default::default(),
        login: web_auth::Login::for_tests(),
        oauth_origin: None,
        embedder: None,
    };
    let id = uuid::Uuid::new_v4().simple().to_string();
    let (sam, alex, w) = (format!("sam-{id}"), format!("alex-{id}"), format!("w-{id}"));
    let create = Command::Workspace { title: w.clone() };
    store::execute(&pool, &w, &sam, "create", &create).await?;
    let member = Command::Member {
        id: alex.clone(),
        role: "human".into(),
    };
    store::execute(&pool, &w, &sam, "add-alex", &member).await?;
    let origin = "http://joppa.test";
    let mut cookies = vec![];
    for actor in [&sam, &alex] {
        let token = format!("{}{}", uuid::Uuid::new_v4().simple(), id);
        sessions::create(&pool, &token, actor, None).await?;
        cookies.push(format!("joppa_session={token}"));
    }
    let (sam_cookie, alex_cookie) = (&cookies[0], &cookies[1]);
    // S and alex read W; the member list and the grants of W are the evidence.
    let snapshot = || async {
        let mut members = vec![];
        for cookie in [sam_cookie, alex_cookie] {
            let Json(index) = read(
                State(app.clone()),
                Path(w.clone()),
                browser(Some(cookie), origin),
            )
            .await
            .unwrap();
            members.push(index["index"]["members"].clone());
        }
        let grants: Vec<String> = app
            .pool
            .get()
            .await
            .unwrap()
            .query(
                "SELECT email FROM joppa.email_grants WHERE workspace=$1 ORDER BY email",
                &[&w],
            )
            .await
            .unwrap()
            .iter()
            .map(|row| row.get(0))
            .collect();
        (members, grants)
    };
    let before = snapshot().await;
    assert_eq!(
        before.0,
        vec![json!({sam.clone():"human", alex.clone():"human"}); 2]
    );
    assert!(before.1.is_empty());
    // Other tests of this database bind maria@example.com, so the emails have the test ID.
    let (maria_email, lena_email) = (
        format!("maria-{id}@example.com"),
        format!("lena-{id}@example.com"),
    );
    let (maria_sub, lena_sub) = (format!("google-maria-{id}"), format!("google-lena-{id}"));
    let user = |sub: &str| format!("user-{}", joppa::digest_hex::sha256_hex(sub.as_bytes()));
    let (maria, lena) = (user(&maria_sub), user(&lena_sub));
    let members = "SELECT count(*) FROM joppa.state_entries WHERE section='members' AND key=$1";
    let bound = "SELECT count(*) FROM joppa.google_identities WHERE actor=$1";
    let sessions_of = "SELECT count(*) FROM joppa.browser_sessions WHERE actor=$1";
    // alex, a human member who did not create W, adds maria.
    let Json(granted) = grant_email(
        State(app.clone()),
        browser(Some(alex_cookie), origin),
        Json(EmailGrant {
            workspace: w.clone(),
            email: maria_email.clone(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(granted, json!({"workspace":w,"email":maria_email}));
    // The agent bot sends the grant of lena with its credential and is refused.
    let bot = format!("bot-{id}");
    store::enroll_service(&pool, &w, &sam, &bot, "agent").await?;
    let refused = store::grant_email(&pool, &w, &bot, &lena_email)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("a human member of the workspace is required"),
        "{refused}"
    );
    let after = snapshot().await;
    assert_eq!(after.1, vec![maria_email.clone()]);
    // maria reads W after her Google sign-in, as a member with the role human.
    let (headers, Json(ready)) = sign_in(&app, &maria_sub, &maria_email).await.unwrap();
    assert_eq!(ready["actor"], maria.as_str());
    let Json(index) = read(
        State(app.clone()),
        Path(w.clone()),
        browser(Some(&session(&headers)), origin),
    )
    .await
    .unwrap();
    assert_eq!(index["index"]["members"][&maria], "human", "{index}");
    // lena has no grant: her sign-in gives "No workspace access" and leaves no row.
    let (status, Json(error)) = sign_in(&app, &lena_sub, &lena_email).await.unwrap_err();
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error, json!({"error":"No workspace access"}));
    for sql in [members, bound, sessions_of] {
        assert_eq!(count(&app, sql, &lena).await, 0);
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated test database"]
async fn owner_adds_a_blank_or_malformed_email_and_w_does_not_change() -> Result<()> {
    let app = test_app().await?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let (sam, w) = (format!("sam-{id}"), format!("w-{id}"));
    create(&app.pool, &w, &sam).await?;
    let sam_cookie = session_for(&app.pool, &sam).await?;
    let before = members_and_grants(&app, &w, &sam_cookie).await;
    assert_eq!(before, (json!({sam.clone():"human"}), vec![]));
    for input in ["  ", "maria-at-example"] {
        let reply = plain(add_email(&app, &sam_cookie, &w, input).await);
        let refused = json!({"error":"a valid email is required"});
        assert_eq!(
            reply,
            Err((StatusCode::UNPROCESSABLE_ENTITY, refused)),
            "input {input:?}"
        );
        let after = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(after, before, "input {input:?} changed W; reply {reply:?}");
    }
    Ok(())
}
