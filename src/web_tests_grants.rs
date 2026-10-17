// Grants at sign-in: a database fault, and newly granted workspaces.
// Included into web_tests.rs, so the test paths stay web::tests::<name>.
// owner-adds-members-ac10
#[tokio::test]
#[ignore = "requires isolated test database and a role that may create roles"]
async fn a_grant_fault_at_sign_in_answers_503_with_no_database_text() -> Result<()> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let app = test_app().await?;
    let pool = &app.pool;
    let (owner, w, email) = (
        format!("owner-{id}"),
        format!("w-{id}"),
        format!("fault-{id}@example.com"),
    );
    create(pool, &w, &owner).await?;
    let owner_cookie = session_for(pool, &owner).await?;
    plain(add_email(&app, &owner_cookie, &w, &email).await).unwrap();
    let (headers, _) = sign_in(&app, &format!("google-fault-{id}"), &email)
        .await
        .unwrap();
    let cookie = session(&headers);
    // This role reads every table but the email grants, so the grant read fails.
    let role = format!("joppa_fault_{id}");
    pool.get()
        .await?
        .batch_execute(&format!(
            "CREATE ROLE {role} NOLOGIN; GRANT {role} TO CURRENT_USER;
             GRANT USAGE ON SCHEMA joppa TO {role};
             GRANT SELECT ON ALL TABLES IN SCHEMA joppa TO {role};
             REVOKE SELECT ON joppa.email_grants FROM {role};"
        ))
        .await?;
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let separator = if url.contains('?') { '&' } else { '?' };
    let faulty = App {
        pool: store::pool(&format!("{url}{separator}options=-c%20role%3D{role}"))?,
        ..app.clone()
    };
    let page = me(State(faulty.clone()), browser(Some(&cookie), ORIGIN)).await;
    let api = list(State(faulty.clone()), browser(Some(&cookie), ORIGIN)).await;
    drop(faulty);
    pool.get()
        .await?
        .batch_execute(&format!(
            "REVOKE {role} FROM CURRENT_USER; DROP OWNED BY {role}; DROP ROLE {role}"
        ))
        .await?;
    for reply in [page, api] {
        let (status, Json(body)) = reply.unwrap_err();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            body,
            json!({"error":"sign-in grants are not available, try again"})
        );
    }
    // The same session works again once the grants are readable.
    assert!(
        me(State(app.clone()), browser(Some(&cookie), ORIGIN))
            .await
            .is_ok()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated test database"]
async fn a_signed_in_person_reads_a_newly_granted_workspace_after_the_next_page_load() -> Result<()>
{
    let id = uuid::Uuid::new_v4().simple().to_string();
    let sub = format!("google-alex-{id}");
    let alex = derived(&sub);
    let alex_token = format!("alex-token-{id}");
    let app = App {
        identities: Arc::new(BTreeMap::from([(alex_token.clone(), alex.clone())])),
        ..test_app().await?
    };
    let pool = &app.pool;
    // This test owns alex@example.com in the shared test database.
    let conn = pool.get().await?;
    conn.execute(
        "DELETE FROM joppa.email_grants WHERE email='alex@example.com'",
        &[],
    )
    .await?;
    conn.execute(
        "DELETE FROM joppa.google_identities WHERE email='alex@example.com'",
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
    // Alex signs in with Google through a grant on another workspace.
    plain(add_email(&app, &olga_cookie, &home, "alex@example.com").await).unwrap();
    let (headers, Json(ready)) = sign_in(&app, &sub, "alex@example.com").await.unwrap();
    assert_eq!(ready["actor"], alex.as_str());
    let alex_cookie = session(&headers);
    assert_eq!(scalar(&app, MEMBER_OF, &[&w, &alex]).await, 0);
    // S grants W. Token requests for alex and for an agent enroll nobody.
    plain(add_email(&app, &sam_cookie, &w, "alex@example.com").await).unwrap();
    let granted = members_and_grants(&app, &w, &sam_cookie).await;
    assert_eq!(granted.0, json!({sam.clone():"human"}));
    let mut bearer = HeaderMap::new();
    bearer.insert(
        header::AUTHORIZATION,
        format!("Bearer {alex_token}").parse()?,
    );
    assert_eq!(listed(&app, bearer.clone()).await, vec![home.clone()]);
    let refused = plain(read(State(app.clone()), Path(w.clone()), bearer).await);
    assert_eq!(refused.unwrap_err().0, StatusCode::NOT_FOUND);
    let agent = store::create_agent_token(pool, &control, &sam, &format!("bot-{id}"), None).await?;
    let mut bearer = HeaderMap::new();
    let token = agent["token"].as_str().unwrap();
    bearer.insert(header::AUTHORIZATION, format!("Bearer {token}").parse()?);
    assert_eq!(listed(&app, bearer).await, vec![control.clone()]);
    assert_eq!(members_and_grants(&app, &w, &sam_cookie).await, granted);
    // Alex reloads twice with the same session; only the first reload enrolls.
    let events = "SELECT count(*) FROM joppa.events WHERE workspace=$1 AND actor='joppa-access' AND request_id=$2";
    let request = format!("enroll-{alex}");
    for reload in 1..=2 {
        let Json(me_reply) = me(State(app.clone()), browser(Some(&alex_cookie), ORIGIN))
            .await
            .unwrap();
        assert_eq!(me_reply["actor"], alex.as_str(), "reload {reload}");
        assert_eq!(me_reply["email"], "alex@example.com", "reload {reload}");
        let mut ids = listed(&app, browser(Some(&alex_cookie), ORIGIN)).await;
        ids.sort();
        let mut expected = vec![home.clone(), w.clone()];
        expected.sort();
        assert_eq!(ids, expected, "reload {reload}");
        let (members, grants) = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(
            members,
            json!({sam.clone():"human", alex.clone():"human"}),
            "reload {reload}"
        );
        assert_eq!(grants, granted.1, "reload {reload}");
        assert_eq!(
            scalar(&app, events, &[&w, &request]).await,
            1,
            "reload {reload}"
        );
        assert_eq!(
            scalar(&app, SESSIONS_OF, &[&alex]).await,
            1,
            "reload {reload}"
        );
    }
    let (status, error) = plain(
        read(
            State(app.clone()),
            Path(control.clone()),
            browser(Some(&alex_cookie), ORIGIN),
        )
        .await,
    )
    .unwrap_err();
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error, json!({"error":"Workspace not found"}));
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated test database"]
async fn a_configured_user_without_a_google_identity_reads_a_newly_granted_workspace() -> Result<()>
{
    let id = uuid::Uuid::new_v4().simple().to_string();
    let (casey, email) = (format!("casey-{id}"), format!("casey-{id}@example.test"));
    let mut app = test_app().await?;
    app.login.users.insert(email.clone(), casey.clone());
    let pool = &app.pool;
    let sam = format!("sam-{id}");
    let (earlier, w, control) = (
        format!("earlier-{id}"),
        format!("w-{id}"),
        format!("control-{id}"),
    );
    create(pool, &earlier, &casey).await?;
    create(pool, &w, &sam).await?;
    create(pool, &control, &sam).await?;
    let (sam_cookie, casey_cookie) = (
        session_for(pool, &sam).await?,
        session_for(pool, &casey).await?,
    );
    let casey_reads = || browser(Some(&casey_cookie), ORIGIN);
    assert_eq!(listed(&app, casey_reads()).await, vec![earlier.clone()]);
    plain(add_email(&app, &sam_cookie, &w, &email).await).unwrap();
    // Casey reloads twice with the same session; only the first reload enrolls.
    let events = "SELECT count(*) FROM joppa.events WHERE workspace=$1 AND actor='joppa-access' AND request_id=$2";
    let request = format!("enroll-{casey}");
    for reload in 1..=2 {
        let Json(me_reply) = me(State(app.clone()), casey_reads()).await.unwrap();
        assert_eq!(me_reply["actor"], casey.as_str(), "reload {reload}");
        let mut ids = listed(&app, casey_reads()).await;
        ids.sort();
        let mut expected = vec![earlier.clone(), w.clone()];
        expected.sort();
        assert_eq!(ids, expected, "reload {reload}");
        let (members, _) = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(
            members,
            json!({sam.clone():"human", casey.clone():"human"}),
            "reload {reload}"
        );
        assert_eq!(
            scalar(&app, events, &[&w, &request]).await,
            1,
            "reload {reload}"
        );
        assert_eq!(
            scalar(&app, SESSIONS_OF, &[&casey]).await,
            1,
            "reload {reload}"
        );
    }
    let bound = "SELECT count(*) FROM joppa.google_identities WHERE actor=$1 OR email=$2";
    assert_eq!(scalar(&app, bound, &[&casey, &email]).await, 0);
    let (status, error) =
        plain(read(State(app.clone()), Path(control.clone()), casey_reads()).await).unwrap_err();
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error, json!({"error":"Workspace not found"}));
    Ok(())
}
