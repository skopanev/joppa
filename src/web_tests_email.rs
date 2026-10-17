// Email grants: the owner grants an email, and that Google account reads only W.
// Included into web_tests.rs, so the test paths stay web::tests::<name>.
#[tokio::test]
#[ignore = "requires isolated test database"]
async fn owner_grants_an_email_and_the_google_account_reads_only_that_workspace() -> Result<()> {
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
    let (sam, other_owner) = (format!("sam-{id}"), format!("olga-{id}"));
    let (w, other) = (format!("granted-{id}"), format!("other-{id}"));
    for (workspace, owner) in [(&w, &sam), (&other, &other_owner)] {
        store::execute(
            &pool,
            workspace,
            owner,
            "create",
            &Command::Workspace {
                title: workspace.clone(),
            },
        )
        .await?;
    }
    let (sam_cookie, olga_cookie) = (
        format!("{id}{}", uuid::Uuid::new_v4().simple()),
        format!("{}{id}", uuid::Uuid::new_v4().simple()),
    );
    sessions::create(&pool, &sam_cookie, &sam, None).await?;
    sessions::create(&pool, &olga_cookie, &other_owner, None).await?;
    let (sam_cookie, olga_cookie) = (
        format!("joppa_session={sam_cookie}"),
        format!("joppa_session={olga_cookie}"),
    );
    let origin = "http://joppa.test";
    let grant = |headers: HeaderMap, workspace: &str| {
        grant_email(
            State(app.clone()),
            headers,
            Json(EmailGrant {
                workspace: workspace.into(),
                email: "Maria@Example.com".into(),
            }),
        )
    };
    let grants = "SELECT count(*) FROM joppa.email_grants WHERE email=$1";
    // Non-owners, tokens and cross-origin requests record no grant.
    let refused = grant(browser(Some(&olga_cookie), origin), &w)
        .await
        .unwrap_err();
    assert_eq!(refused.1.0["error"], "workspace owner required");
    let mut token = browser(Some(&sam_cookie), origin);
    token.insert(header::AUTHORIZATION, "Bearer x".parse().unwrap());
    assert_eq!(grant(token, &w).await.unwrap_err().0, StatusCode::FORBIDDEN);
    let cross = grant(browser(Some(&sam_cookie), "https://evil.test"), &w).await;
    assert_eq!(cross.unwrap_err().0, StatusCode::FORBIDDEN);
    let anonymous = grant(browser(None, origin), &w).await;
    assert_eq!(anonymous.unwrap_err().0, StatusCode::UNAUTHORIZED);
    assert_eq!(count(&app, grants, "maria@example.com").await, 0);
    // A verified email with no grant, rule or user entry cannot sign in.
    let refused = sign_in(&app, "google-maria", "maria@example.com").await;
    assert_eq!(refused.unwrap_err().0, StatusCode::FORBIDDEN);
    let derived = format!("user-{:x}", sha2::Sha256::digest(b"google-maria"));
    let members = "SELECT count(*) FROM joppa.state_entries WHERE section='members' AND key=$1";
    let bound = "SELECT count(*) FROM joppa.google_identities WHERE actor=$1";
    let sessions_of = "SELECT count(*) FROM joppa.browser_sessions WHERE actor=$1";
    for sql in [members, bound, sessions_of] {
        assert_eq!(count(&app, sql, &derived).await, 0);
    }
    // The owner grants W twice; one lowercase grant remains.
    for _ in 0..2 {
        let Json(granted) = grant(browser(Some(&sam_cookie), origin), &w).await.unwrap();
        assert_eq!(granted, json!({"workspace":w,"email":"maria@example.com"}));
    }
    assert_eq!(count(&app, grants, "maria@example.com").await, 1);
    let Json(listed) = list_email_grants(State(app.clone()), browser(Some(&sam_cookie), origin))
        .await
        .unwrap();
    assert_eq!(
        listed,
        json!({"items":[{"id":w,"title":w,"emails":["maria@example.com"]}]})
    );
    // Maria signs in twice; the second sign-in adds no membership.
    let (headers, Json(ready)) = sign_in(&app, "google-maria", "maria@example.com")
        .await
        .unwrap();
    assert_eq!(ready["actor"], derived.as_str());
    let maria = session(&headers);
    assert_eq!(count(&app, sessions_of, &derived).await, 1);
    let (_, Json(again)) = sign_in(&app, "google-maria", "maria@example.com")
        .await
        .unwrap();
    assert_eq!(again["actor"], derived.as_str());
    assert_eq!(count(&app, members, &derived).await, 1);
    let Json(list_reply) = list(State(app.clone()), browser(Some(&maria), origin))
        .await
        .unwrap();
    assert_eq!(list_reply, json!([{"id":w,"title":w}]));
    let Json(index) = read(
        State(app.clone()),
        Path(w.clone()),
        browser(Some(&maria), origin),
    )
    .await
    .unwrap();
    assert_eq!(index["index"]["owner"], sam.as_str());
    assert_eq!(
        index["index"]["members"],
        json!({sam.clone():"human", derived.clone():"human"})
    );
    let (status, Json(error)) = read(
        State(app.clone()),
        Path(other.clone()),
        browser(Some(&maria), origin),
    )
    .await
    .unwrap_err();
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error, json!({"error":"Workspace not found"}));
    // Another Google account cannot claim the bound email.
    let stolen = sign_in(&app, "google-other", "maria@example.com").await;
    assert_eq!(stolen.unwrap_err().0, StatusCode::FORBIDDEN);
    Ok(())
}
