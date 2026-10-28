// Domain grants that need the hosted domain (hd) of the Google session.
// Included into web_tests.rs, so the test paths stay web::tests::<name>.
/// AC9 tests own these domains and emails in the shared test database.
/// They start and end without their grants and Google bindings.
async fn forget_hd_fixtures(app: &App, emails: &[&str]) -> Result<()> {
    let conn = app.pool.get().await?;
    conn.execute(
        "DELETE FROM joppa.domain_grants WHERE domain IN ('healthium.one','gmail.com')",
        &[],
    )
    .await?;
    let emails: Vec<String> = emails.iter().map(|e| e.to_string()).collect();
    conn.execute(
        "DELETE FROM joppa.email_grants WHERE email=ANY($1)",
        &[&emails],
    )
    .await?;
    conn.execute(
        "DELETE FROM joppa.google_identities WHERE email=ANY($1)",
        &[&emails],
    )
    .await?;
    Ok(())
}

/// The stored hd of the browser session in this cookie.
async fn session_hd(app: &App, cookie: &str) -> Option<String> {
    let token = cookie.strip_prefix("joppa_session=").unwrap();
    let hash = joppa::digest_hex::sha256_hex(token.as_bytes());
    let conn = app.pool.get().await.unwrap();
    conn.query_one(
        "SELECT hd FROM joppa.browser_sessions WHERE token_hash=$1",
        &[&hash],
    )
    .await
    .unwrap()
    .get(0)
}

/// A page load with this cookie, then the workspaces that it lists.
async fn reload(app: &App, cookie: &str, actor: &str) -> Vec<String> {
    let Json(me_reply) = me(State(app.clone()), browser(Some(cookie), ORIGIN))
        .await
        .unwrap();
    assert_eq!(me_reply["actor"], actor);
    let mut ids = listed(app, browser(Some(cookie), ORIGIN)).await;
    ids.sort();
    ids
}

// owner-adds-members-ac9
#[tokio::test]
#[ignore = "requires isolated test database"]
async fn domain_grant_needs_hd_lena_reads_w_and_people_without_that_hd_do_not() -> Result<()> {
    let mut app = test_app().await?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let casey_email = format!("casey-{id}@healthium.one");
    let emails = [
        "lena@healthium.one",
        "ex@healthium.one",
        "pat@gmail.com",
        "max@healthium.one",
    ];
    forget_hd_fixtures(&app, &emails).await?;
    let casey = format!("casey-{id}");
    app.login.users.insert(casey_email.clone(), casey.clone());
    let pool = app.pool.clone();
    let (sam, olga) = (format!("sam-{id}"), format!("olga-{id}"));
    let (home, w, w2) = (format!("home-{id}"), format!("w-{id}"), format!("w2-{id}"));
    create(&pool, &home, &olga).await?;
    create(&pool, &w, &sam).await?;
    create(&pool, &w2, &sam).await?;
    let (sam_cookie, olga_cookie) = (
        session_for(&pool, &sam).await?,
        session_for(&pool, &olga).await?,
    );
    plain(add_domain(&app, &sam_cookie, &w, "healthium.one").await).unwrap();
    plain(add_domain(&app, &sam_cookie, &w2, "gmail.com").await).unwrap();
    // Lena signs in with the company account.
    let lena_sub = format!("google-lena-{id}");
    let lena = derived(&lena_sub);
    let (headers, Json(ready)) = sign_in_hd(&app, &lena_sub, "lena@healthium.one", HEALTHIUM)
        .await
        .unwrap();
    assert_eq!(ready["actor"], lena.as_str());
    let lena_cookie = session(&headers);
    assert_eq!(session_hd(&app, &lena_cookie).await.as_deref(), HEALTHIUM);
    assert_eq!(reload(&app, &lena_cookie, &lena).await, vec![w.clone()]);
    let (members, _) = members_and_grants(&app, &w, &sam_cookie).await;
    assert_eq!(members, json!({sam.clone():"human", lena.clone():"human"}));
    // Without hd, or with another hd, the domain grant admits nobody.
    let people = [
        ("ex@healthium.one", None),
        ("pat@gmail.com", None),
        ("max@healthium.one", Some("other-hd.test")),
    ];
    for (email, hd) in people {
        let sub = format!("google-{email}-{id}");
        let (status, Json(error)) = sign_in_hd(&app, &sub, email, hd).await.unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN, "{email}");
        assert_eq!(error, json!({"error":"No workspace access"}), "{email}");
        let members = "SELECT count(*) FROM joppa.state_entries WHERE section='members' AND key=$1";
        let bound = "SELECT count(*) FROM joppa.google_identities WHERE actor=$1";
        for sql in [members, bound, SESSIONS_OF] {
            assert_eq!(
                scalar(&app, sql, &[&derived(&sub)]).await,
                0,
                "{email}: {sql}"
            );
        }
    }
    // An email grant on home lets them sign in. They still do not read W or W2.
    let mut outsiders = vec![];
    for (email, hd) in people {
        plain(add_email(&app, &olga_cookie, &home, email).await).unwrap();
        let actor = derived(&format!("google-{email}-{id}"));
        let sub = format!("google-{email}-{id}");
        let (headers, _) = sign_in_hd(&app, &sub, email, hd).await.unwrap();
        let cookie = session(&headers);
        assert_eq!(
            reload(&app, &cookie, &actor).await,
            vec![home.clone()],
            "{email}"
        );
        for workspace in [&w, &w2] {
            let refused = plain(
                read(
                    State(app.clone()),
                    Path(workspace.clone()),
                    browser(Some(&cookie), ORIGIN),
                )
                .await,
            );
            assert_eq!(refused.unwrap_err().0, StatusCode::NOT_FOUND, "{email}");
        }
        outsiders.push(actor);
    }
    // A configured actor without a bound identity gets no domain grant, also with hd.
    let casey_cookie = format!("joppa_session={}{}", id, uuid::Uuid::new_v4().simple());
    let token = casey_cookie.strip_prefix("joppa_session=").unwrap();
    sessions::create(&pool, token, &casey, HEALTHIUM).await?;
    assert_eq!(
        reload(&app, &casey_cookie, &casey).await,
        Vec::<String>::new()
    );
    let (w_members, _) = members_and_grants(&app, &w, &sam_cookie).await;
    assert_eq!(
        w_members,
        json!({sam.clone():"human", lena.clone():"human"})
    );
    let (w2_members, _) = members_and_grants(&app, &w2, &sam_cookie).await;
    assert_eq!(w2_members, json!({sam.clone():"human"}));
    for actor in outsiders.iter().chain([&casey]) {
        assert_eq!(scalar(&app, MEMBER_OF, &[&w, actor]).await, 0, "{actor}");
        assert_eq!(scalar(&app, MEMBER_OF, &[&w2, actor]).await, 0, "{actor}");
    }
    forget_hd_fixtures(&app, &emails).await
}

// owner-adds-members-ac9
#[tokio::test]
#[ignore = "requires isolated test database"]
async fn domain_grant_needs_hd_a_reload_and_an_older_session_do_not_enroll_ex() -> Result<()> {
    let app = test_app().await?;
    let pool = &app.pool;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let emails = ["ex@healthium.one"];
    forget_hd_fixtures(&app, &emails).await?;
    let (sam, olga) = (format!("sam-{id}"), format!("olga-{id}"));
    let (home, w) = (format!("home-{id}"), format!("w-{id}"));
    create(pool, &home, &olga).await?;
    create(pool, &w, &sam).await?;
    let (sam_cookie, olga_cookie) = (
        session_for(pool, &sam).await?,
        session_for(pool, &olga).await?,
    );
    // Ex signs in with no hd through an email grant on home.
    plain(add_email(&app, &olga_cookie, &home, "ex@healthium.one").await).unwrap();
    let ex_sub = format!("google-ex-{id}");
    let ex = derived(&ex_sub);
    let (headers, _) = sign_in(&app, &ex_sub, "ex@healthium.one").await.unwrap();
    let ex_cookie = session(&headers);
    assert_eq!(session_hd(&app, &ex_cookie).await, None);
    // A session made before this rule has a NULL hd.
    let older = format!("{}{id}", uuid::Uuid::new_v4().simple());
    pool.get().await?.execute(
        "INSERT INTO joppa.browser_sessions(token_hash,actor,expires_at) VALUES($1,$2,clock_timestamp()+interval '1 hour')",
        &[&joppa::digest_hex::sha256_hex(older.as_bytes()), &ex],
    ).await?;
    let older = format!("joppa_session={older}");
    assert_eq!(session_hd(&app, &older).await, None);
    // S adds healthium.one to W. Ex reloads with each session.
    plain(add_domain(&app, &sam_cookie, &w, "healthium.one").await).unwrap();
    for cookie in [&ex_cookie, &older, &ex_cookie] {
        assert_eq!(reload(&app, cookie, &ex).await, vec![home.clone()]);
        let (members, _) = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(members, json!({sam.clone():"human"}));
    }
    assert_eq!(scalar(&app, MEMBER_OF, &[&w, &ex]).await, 0);
    forget_hd_fixtures(&app, &emails).await
}
