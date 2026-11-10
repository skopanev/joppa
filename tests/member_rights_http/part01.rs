// Part 1 of tests/member_rights_http.rs, included there, so the test names stay unchanged.

/// GET /api/access of alex and of bob gives the same item of W. Gives the item of alex.
async fn same_access(w: &World, alex: &Who<'_>, step: &str) -> Result<Value> {
    let of_alex = access_item(w, alex).await?;
    let of_bob = access_item(w, &Who::Token(&w.owner)).await?;
    ensure!(of_alex == of_bob, "{step}: alex {of_alex} / bob {of_bob}");
    Ok(of_alex)
}

// requirement:member-rights: each human member uses the access endpoints.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_human_member_who_did_not_create_the_workspace_uses_each_access_endpoint() -> Result<()> {
    let w = record_world::world(&[]).await?;
    joppa::store::grant_email(&w.pool, &w.ws, "owner", "lena@example.com").await?;
    // The domain has the test ID, so the people of other tests get no access to W.
    let domain = format!("example-{}.org", uuid::Uuid::new_v4().simple());
    let cookie = session(&w, "alex").await?;
    let alex = Who::Session(&cookie);
    let ws = w.ws.clone();
    let start = same_access(&w, &alex, "start").await?;
    ensure!(start["emails"] == json!(["lena@example.com"]), "{start}");
    let steps = [
        (
            "/api/email-grants",
            json!({"workspace":ws,"email":"maria@example.com"}),
        ),
        (
            "/api/domain-grants",
            json!({"workspace":ws,"domain":domain}),
        ),
        (
            "/api/access/remove",
            json!({"workspace":ws,"email":"maria@example.com"}),
        ),
        (
            "/api/access/remove",
            json!({"workspace":ws,"domain":domain}),
        ),
        (
            "/api/access/remove",
            json!({"workspace":ws,"email":"lena@example.com"}),
        ),
        (
            "/api/access/remove",
            json!({"workspace":ws,"member":"another-member"}),
        ),
    ];
    for (path, body) in steps {
        accepted(&w, &alex, path, Some(body.clone())).await?;
        same_access(&w, &alex, &format!("{path} {body}")).await?;
    }
    let references = format!("/api/workspaces/{ws}/references");
    let listed = accepted(&w, &alex, &references, None).await?;
    ensure!(listed["workspaces"] == json!([]), "{listed}");
    let item = same_access(&w, &alex, "end").await?;
    let ids: Vec<&Value> = item["members"]
        .as_array()
        .context("members")?
        .iter()
        .map(|m| &m["id"])
        .collect();
    ensure!(ids == [&json!("alex"), &json!("owner")], "{item}");
    ensure!(
        item["emails"] == json!([]) && item["domains"] == json!([]),
        "{item}"
    );
    // The removed member another-member has no access to W.
    let gone = w.mcp(&w.other, "joppa_list", json!({})).await?;
    ensure!(!format!("{gone:?}").contains(&ws), "{gone:?}");

    // bot sends the same requests with its credential: each one is refused, and W keeps its
    // grants and members.
    let before = access_item(&w, &Who::Token(&w.owner)).await?;
    let bot = Who::Token(&w.bot);
    for (path, body) in [
        (
            "/api/email-grants",
            json!({"workspace":ws,"email":"maria@example.com"}),
        ),
        (
            "/api/domain-grants",
            json!({"workspace":ws,"domain":domain}),
        ),
        (
            "/api/access/remove",
            json!({"workspace":ws,"email":"maria@example.com"}),
        ),
        (
            "/api/access/remove",
            json!({"workspace":ws,"domain":domain}),
        ),
        (
            "/api/access/remove",
            json!({"workspace":ws,"member":"alex"}),
        ),
    ] {
        denied(&w, &bot, path, Some(body)).await?;
    }
    denied(&w, &bot, &references, None).await?;
    let (code, reply) = send(&w, &bot, "/api/access", None).await?;
    no_owner_words(&reply.to_string())?;
    ensure!(
        code != 200 || !reply.to_string().contains(&ws),
        "the agent read the access of W: {reply}"
    );
    // The store gives the refusal of the agent its text.
    let refused = joppa::store::grant_email(&w.pool, &ws, "bot", "maria@example.com").await;
    ensure!(
        refused.as_ref().is_err_and(|e| e
            .to_string()
            .contains("a human member of the workspace is required")),
        "{refused:?}"
    );
    let after = access_item(&w, &Who::Token(&w.owner)).await?;
    ensure!(after == before, "the agent changed W: {before} -> {after}");
    Ok(())
}
