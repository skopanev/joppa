// Domain grants: the owner adds a domain, refusals and repeats.
// Included into web_tests.rs, so the test paths stay web::tests::<name>.
// owner-adds-members-ac9
#[tokio::test]
#[ignore = "requires isolated test database"]
async fn owner_adds_a_domain_and_its_google_people_read_w_as_members() -> Result<()> {
    let app = test_app().await?;
    let pool = &app.pool;
    let id = uuid::Uuid::new_v4().simple().to_string();
    // This test owns healthium.one in the shared test database.
    let conn = pool.get().await?;
    conn.execute(
        "DELETE FROM joppa.domain_grants WHERE domain='healthium.one'",
        &[],
    )
    .await?;
    conn.execute(
        "DELETE FROM joppa.email_grants WHERE email IN ('maria@healthium.one','lena@healthium.one')",
        &[],
    )
    .await?;
    drop(conn);
    let (sam, olga) = (format!("sam-{id}"), format!("olga-{id}"));
    let (home, w, control) = (
        format!("home-{id}"),
        format!("w-{id}"),
        format!("control-{id}"),
    );
    create(pool, &home, &olga).await?;
    create(pool, &w, &sam).await?;
    create(pool, &control, &sam).await?;
    let (sam_cookie, olga_cookie) = (
        session_for(pool, &sam).await?,
        session_for(pool, &olga).await?,
    );
    // Maria is signed in through an email grant on another workspace.
    plain(add_email(&app, &olga_cookie, &home, "maria@healthium.one").await).unwrap();
    let maria_sub = format!("google-maria-{id}");
    let maria = derived(&maria_sub);
    let (headers, _) = sign_in_hd(&app, &maria_sub, "maria@healthium.one", HEALTHIUM)
        .await
        .unwrap();
    let maria_cookie = session(&headers);
    assert_eq!(scalar(&app, MEMBER_OF, &[&w, &maria]).await, 0);
    // S adds the domain twice in another case; one lowercase grant remains.
    for _ in 0..2 {
        let reply = plain(add_domain(&app, &sam_cookie, &w, "HEALTHIUM.one").await);
        assert_eq!(reply, Ok(json!({"workspace":w,"domain":"healthium.one"})));
        let state = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(
            state,
            (
                json!({sam.clone():"human"}),
                vec!["domain healthium.one".into()]
            )
        );
    }
    let Json(domains) = list_domain_grants(State(app.clone()), browser(Some(&sam_cookie), ORIGIN))
        .await
        .unwrap();
    let mut expected = vec![
        json!({"id":control,"title":control,"domains":[]}),
        json!({"id":w,"title":w,"domains":["healthium.one"]}),
    ];
    expected.sort_by_key(|item| item["id"].as_str().unwrap().to_owned());
    assert_eq!(domains, json!({"items": expected}));
    // Maria reads the board again with the same session.
    let Json(me_reply) = me(State(app.clone()), browser(Some(&maria_cookie), ORIGIN))
        .await
        .unwrap();
    assert_eq!(me_reply["actor"], maria.as_str());
    let mut ids = listed(&app, browser(Some(&maria_cookie), ORIGIN)).await;
    ids.sort();
    let mut expected = vec![home.clone(), w.clone()];
    expected.sort();
    assert_eq!(ids, expected);
    assert_eq!(scalar(&app, SESSIONS_OF, &[&maria]).await, 1);
    // Lena signs in with Google for the first time.
    let lena_sub = format!("google-lena-{id}");
    let lena = derived(&lena_sub);
    let (headers, Json(ready)) = sign_in_hd(&app, &lena_sub, "lena@healthium.one", HEALTHIUM)
        .await
        .unwrap();
    assert_eq!(ready["actor"], lena.as_str());
    let lena_cookie = session(&headers);
    assert_eq!(scalar(&app, SESSIONS_OF, &[&lena]).await, 1);
    assert_eq!(
        listed(&app, browser(Some(&lena_cookie), ORIGIN)).await,
        vec![w.clone()]
    );
    let (members, _) = members_and_grants(&app, &w, &sam_cookie).await;
    assert_eq!(
        members,
        json!({sam.clone():"human", maria.clone():"human", lena.clone():"human"})
    );
    let (status, error) = plain(
        read(
            State(app.clone()),
            Path(control.clone()),
            browser(Some(&lena_cookie), ORIGIN),
        )
        .await,
    )
    .unwrap_err();
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error, json!({"error":"Workspace not found"}));
    // A verified email in another domain has no grant or rule.
    let ivan_sub = format!("google-ivan-{id}");
    let ivan = derived(&ivan_sub);
    let (status, Json(error)) = sign_in(&app, &ivan_sub, &format!("ivan@other-{id}.test"))
        .await
        .unwrap_err();
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error, json!({"error":"No workspace access"}));
    let members = "SELECT count(*) FROM joppa.state_entries WHERE section='members' AND key=$1";
    let bound = "SELECT count(*) FROM joppa.google_identities WHERE actor=$1";
    for sql in [members, bound, SESSIONS_OF] {
        assert_eq!(scalar(&app, sql, &[&ivan]).await, 0, "{sql}");
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated test database"]
async fn a_non_owner_or_an_invalid_domain_leaves_w_unchanged() -> Result<()> {
    let app = test_app().await?;
    let pool = &app.pool;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let (sam, alex, w) = (format!("sam-{id}"), format!("alex-{id}"), format!("w-{id}"));
    create(pool, &w, &sam).await?;
    let member = Command::Member {
        id: alex.clone(),
        role: "human".into(),
    };
    store::execute(pool, &w, &sam, "add-alex", &member).await?;
    let (sam_cookie, alex_cookie) = (
        session_for(pool, &sam).await?,
        session_for(pool, &alex).await?,
    );
    let before = members_and_grants(&app, &w, &sam_cookie).await;
    assert_eq!(
        before,
        (json!({sam.clone():"human", alex.clone():"human"}), vec![])
    );
    let reply = plain(add_domain(&app, &alex_cookie, &w, "healthium.one").await);
    let owner_required = json!({"error":"workspace owner required"});
    assert_eq!(reply, Err((StatusCode::CONFLICT, owner_required)));
    assert_eq!(members_and_grants(&app, &w, &sam_cookie).await, before);
    let invalid = [
        "healthium",
        "",
        "  ",
        "@healthium.one",
        "maria@healthium.one",
        "health ium.one",
        " healthium.one",
    ];
    for input in invalid {
        let reply = plain(add_domain(&app, &sam_cookie, &w, input).await);
        let refused = json!({"error":"a valid email domain is required"});
        assert_eq!(
            reply,
            Err((StatusCode::UNPROCESSABLE_ENTITY, refused)),
            "input {input:?}"
        );
        let after = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(after, before, "input {input:?} changed W");
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated test database"]
async fn the_owner_adds_an_existing_member_again_and_w_does_not_change() -> Result<()> {
    let app = test_app().await?;
    let pool = &app.pool;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let (sam, w) = (format!("sam-{id}"), format!("w-{id}"));
    create(pool, &w, &sam).await?;
    let sam_cookie = session_for(pool, &sam).await?;
    plain(add_email(&app, &sam_cookie, &w, "maria@example.com").await).unwrap();
    // The email binds one Google subject. Reuse another test's binding instead of deleting it.
    let bound = pool
        .get()
        .await?
        .query_opt(
            "SELECT google_sub FROM joppa.google_identities WHERE email='maria@example.com'",
            &[],
        )
        .await?
        .map(|row| row.get::<_, String>(0));
    let sub = bound.clone().unwrap_or(format!("google-maria-{id}"));
    let maria = derived(&sub);
    let (_, Json(ready)) = sign_in(&app, &sub, "maria@example.com").await.unwrap();
    assert_eq!(ready["actor"], maria.as_str());
    let before = members_and_grants(&app, &w, &sam_cookie).await;
    assert_eq!(
        before,
        (
            json!({sam.clone():"human", maria.clone():"human"}),
            vec!["maria@example.com".to_owned()]
        )
    );
    let reply = plain(add_email(&app, &sam_cookie, &w, "maria@example.com").await);
    assert_eq!(
        reply,
        Ok(json!({"workspace":w,"email":"maria@example.com"}))
    );
    assert_eq!(members_and_grants(&app, &w, &sam_cookie).await, before);
    // Later tests in this database start without this test's grant and binding.
    let conn = pool.get().await?;
    conn.execute("DELETE FROM joppa.email_grants WHERE workspace=$1", &[&w])
        .await?;
    if bound.is_none() {
        conn.execute(
            "DELETE FROM joppa.google_identities WHERE actor=$1",
            &[&maria],
        )
        .await?;
    }
    Ok(())
}
