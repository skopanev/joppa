// Email grants: refusals for a non-owner and for a malformed email.
// Included into web_tests.rs, so the test paths stay web::tests::<name>.
#[tokio::test]
#[ignore = "requires isolated test database"]
async fn a_member_who_is_not_the_owner_cannot_add_maria_and_she_has_no_workspace_access()
-> Result<()> {
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
    // Maria has no grant, login entry or rule; other tests in this database may grant her.
    pool.get()
        .await?
        .execute(
            "DELETE FROM joppa.email_grants WHERE email='maria@example.com'",
            &[],
        )
        .await?;
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
    let sub = format!("google-maria-{id}");
    let maria = format!("user-{:x}", sha2::Sha256::digest(sub.as_bytes()));
    let members = "SELECT count(*) FROM joppa.state_entries WHERE section='members' AND key=$1";
    let bound = "SELECT count(*) FROM joppa.google_identities WHERE actor=$1";
    let sessions_of = "SELECT count(*) FROM joppa.browser_sessions WHERE actor=$1";
    let refused_sign_in = || async {
        let (status, Json(error)) = sign_in(&app, &sub, "maria@example.com").await.unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(error, json!({"error":"No workspace access"}));
        for sql in [members, bound, sessions_of] {
            assert_eq!(count(&app, sql, &maria).await, 0);
        }
    };
    refused_sign_in().await;
    let refused = grant_email(
        State(app.clone()),
        browser(Some(alex_cookie), origin),
        Json(EmailGrant {
            workspace: w.clone(),
            email: "maria@example.com".into(),
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.1.0, json!({"error":"workspace owner required"}));
    assert_eq!(snapshot().await, before);
    refused_sign_in().await;
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
