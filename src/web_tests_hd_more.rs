// Domain grants with hd: the page-load session, and email grants without hd.
// Included into web_tests.rs, so the test paths stay web::tests::<name>.
// owner-adds-members-ac9
#[tokio::test]
#[ignore = "requires isolated test database"]
async fn domain_grant_needs_hd_of_the_session_that_loads_the_page() -> Result<()> {
    let app = test_app().await?;
    let pool = &app.pool;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let emails = ["kim@healthium.one", "kai@healthium.one"];
    forget_hd_fixtures(&app, &emails).await?;
    let olga = format!("olga-{id}");
    let home = format!("home-{id}");
    create(pool, &home, &olga).await?;
    let olga_cookie = session_for(pool, &olga).await?;
    // Kim signs in with the company hd first. Kai signs in with no hd first.
    for (email, hd_first) in [(emails[0], true), (emails[1], false)] {
        // Each order starts without the domain grant of the other order.
        forget_hd_fixtures(&app, &[]).await?;
        let name = email.split('@').next().unwrap();
        let (sam, w) = (format!("sam-{name}-{id}"), format!("w-{name}-{id}"));
        create(pool, &w, &sam).await?;
        let sam_cookie = session_for(pool, &sam).await?;
        plain(add_email(&app, &olga_cookie, &home, email).await).unwrap();
        let sub = format!("google-{email}-{id}");
        let actor = derived(&sub);
        let mut cookies = vec![];
        for hd in if hd_first {
            [HEALTHIUM, None]
        } else {
            [None, HEALTHIUM]
        } {
            let (headers, _) = sign_in_hd(&app, &sub, email, hd).await.unwrap();
            cookies.push(session(&headers));
        }
        if !hd_first {
            cookies.reverse();
        }
        let (with_hd, without_hd) = (&cookies[0], &cookies[1]);
        assert_eq!(
            session_hd(&app, with_hd).await.as_deref(),
            HEALTHIUM,
            "{email}"
        );
        assert_eq!(session_hd(&app, without_hd).await, None, "{email}");
        assert_eq!(scalar(&app, SESSIONS_OF, &[&actor]).await, 2, "{email}");
        plain(add_domain(&app, &sam_cookie, &w, "healthium.one").await).unwrap();
        // The page load with no hd does not enroll the person.
        assert_eq!(
            reload(&app, without_hd, &actor).await,
            vec![home.clone()],
            "{email}"
        );
        assert_eq!(scalar(&app, MEMBER_OF, &[&w, &actor]).await, 0, "{email}");
        // The page load with the company hd enrolls the person in W as human.
        let mut expected = vec![home.clone(), w.clone()];
        expected.sort();
        assert_eq!(reload(&app, with_hd, &actor).await, expected, "{email}");
        let (members, _) = members_and_grants(&app, &w, &sam_cookie).await;
        assert_eq!(
            members,
            json!({sam.clone():"human", actor.clone():"human"}),
            "{email}"
        );
        assert_eq!(session_hd(&app, without_hd).await, None, "{email}");
    }
    forget_hd_fixtures(&app, &emails).await
}

// owner-adds-members-ac9
#[tokio::test]
#[ignore = "requires isolated test database"]
async fn domain_grant_needs_hd_but_an_email_grant_does_not_and_hd_case_does_not_matter()
-> Result<()> {
    let app = test_app().await?;
    let pool = &app.pool;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let emails = ["ex@healthium.one", "lena@healthium.one"];
    forget_hd_fixtures(&app, &emails).await?;
    let sam = format!("sam-{id}");
    let (w, w2) = (format!("w-{id}"), format!("w2-{id}"));
    create(pool, &w, &sam).await?;
    create(pool, &w2, &sam).await?;
    let sam_cookie = session_for(pool, &sam).await?;
    // S grants the single email. Ex signs in with no hd and reads W.
    plain(add_email(&app, &sam_cookie, &w, "ex@healthium.one").await).unwrap();
    let ex_sub = format!("google-ex-{id}");
    let ex = derived(&ex_sub);
    let (headers, _) = sign_in(&app, &ex_sub, "ex@healthium.one").await.unwrap();
    let ex_cookie = session(&headers);
    assert_eq!(session_hd(&app, &ex_cookie).await, None);
    assert_eq!(reload(&app, &ex_cookie, &ex).await, vec![w.clone()]);
    let read_w = read(
        State(app.clone()),
        Path(w.clone()),
        browser(Some(&ex_cookie), ORIGIN),
    );
    let Json(index) = read_w.await.unwrap();
    assert_eq!(
        index["index"]["members"],
        json!({sam.clone():"human", ex.clone():"human"})
    );
    // A mixed-case hd matches the healthium.one domain grant.
    plain(add_domain(&app, &sam_cookie, &w2, "healthium.one").await).unwrap();
    let lena_sub = format!("google-lena-{id}");
    let lena = derived(&lena_sub);
    let (headers, Json(ready)) =
        sign_in_hd(&app, &lena_sub, "lena@healthium.one", Some("HealthIum.One"))
            .await
            .unwrap();
    assert_eq!(ready["actor"], lena.as_str());
    let lena_cookie = session(&headers);
    assert_eq!(session_hd(&app, &lena_cookie).await.as_deref(), HEALTHIUM);
    assert_eq!(reload(&app, &lena_cookie, &lena).await, vec![w2.clone()]);
    let (members, _) = members_and_grants(&app, &w2, &sam_cookie).await;
    assert_eq!(members, json!({sam.clone():"human", lena.clone():"human"}));
    forget_hd_fixtures(&app, &emails).await
}
