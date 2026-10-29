// Part 15 of tests/reports_http.rs, included there, so the test names stay unchanged.

// settings-agent-tokens-ac1, fixture 2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn settings_agent_tokens_ac1_session_issue_list_and_origin() -> Result<()> {
    let w = world().await?;
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let session = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&pool, &session, "owner", None).await?;
    let root = w.base.split("/api/").next().context("root")?.to_owned();
    let ws = w.base.rsplit('/').next().context("workspace")?.to_owned();
    // Token and revocation rows of W.
    let counts = || async {
        let row = pool
            .get()
            .await?
            .query_one(
                "SELECT (SELECT count(*) FROM joppa.agent_tokens WHERE workspace=$1),
                        (SELECT count(*) FROM joppa.agent_token_revocations r
                         JOIN joppa.agent_tokens t ON t.id=r.token_id WHERE t.workspace=$1)",
                &[&ws],
            )
            .await?;
        anyhow::Ok((row.get::<_, i64>(0), row.get::<_, i64>(1)))
    };
    let reader = json!({"agent":"planner-reader","access":"read"});

    // A cross-origin cookie-session create is refused and stores nothing.
    let before = counts().await?;
    let (code, reply) = session_post(
        &w,
        &session,
        Some("http://evil.example"),
        "/agent-tokens",
        reader.clone(),
    )
    .await?;
    ensure!(
        code == 403 && reply["error"] == "cross-origin request denied",
        "{code} {reply}"
    );
    ensure!(
        counts().await? == before,
        "a cross-origin create stored data"
    );

    // Fixture 2: a blank agent name issues no token.
    let (_, reply) = session_post(
        &w,
        &session,
        Some(&root),
        "/agent-tokens",
        json!({"agent":"","access":"read"}),
    )
    .await?;
    ensure!(
        reply["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("invalid id")),
        "{reply}"
    );
    ensure!(counts().await? == before, "a blank agent stored data");

    // The owner issues a read token from a same-origin cookie session.
    let (code, issued) = session_post(&w, &session, Some(&root), "/agent-tokens", reader).await?;
    ensure!(
        code == 200 && issued["agent"] == "planner-reader" && issued["access"] == "read",
        "{code} {issued}"
    );
    let token = issued["token"].as_str().context("token")?.to_owned();
    ensure!(counts().await? == (before.0 + 1, before.1));

    // The read token reads W, and a write gets the read-only refusal.
    let read = w
        .http
        .get(format!("{}/objects?parent=*", w.base))
        .bearer_auth(&token)
        .send()
        .await?;
    ensure!(read.status().is_success(), "{}", read.status());
    let (code, refused) = save_note(&w, &token, "refused-by-reader").await?;
    ensure!(
        code == 403 && refused["error"] == "read-only token",
        "{code} {refused}"
    );

    // A bearer create with no Origin header issues a token as before.
    let (code, bot) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200 && bot["access"] == "write", "{code} {bot}");
    let bot_id = bot["id"].as_i64().context("id")?;

    // A cross-origin cookie revoke is refused; a same-origin one revokes.
    let revoke = format!("/agent-tokens/{bot_id}/revoke");
    let before = counts().await?;
    let (code, reply) = session_post(
        &w,
        &session,
        Some("http://evil.example"),
        &revoke,
        Value::Null,
    )
    .await?;
    ensure!(
        code == 403 && reply["error"] == "cross-origin request denied",
        "{code} {reply}"
    );
    ensure!(
        counts().await? == before,
        "a cross-origin revoke stored data"
    );
    let (code, reply) = session_post(&w, &session, Some(&root), &revoke, Value::Null).await?;
    ensure!(code == 200 && reply["revoked"] == true, "{code} {reply}");

    // The owner list holds exactly the public fields, with the stored dates.
    let reply = w
        .http
        .get(format!("{}/agent-tokens", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?;
    ensure!(reply.status().as_u16() == 200, "{}", reply.status());
    let listed: Value = reply.json().await?;
    let rows = pool
        .get()
        .await?
        .query(
            "SELECT t.id, t.token_hash, t.created_at::text, r.revoked_at::text
             FROM joppa.agent_tokens t
             LEFT JOIN joppa.agent_token_revocations r ON r.token_id=t.id
             WHERE t.workspace=$1",
            &[&ws],
        )
        .await?;
    let text = listed.to_string();
    ensure!(!text.contains(&token), "the list holds a token value");
    let items = listed["items"].as_array().context("items")?;
    ensure!(items.len() == rows.len(), "{listed}");
    for row in &rows {
        let (id, hash): (i64, String) = (row.get(0), row.get(1));
        ensure!(!text.contains(&hash), "the list holds a token hash");
        let item = items
            .iter()
            .find(|i| i["id"] == id)
            .with_context(|| format!("{id} not listed: {listed}"))?;
        let mut keys: Vec<&str> = item
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, _)| k.as_str())
            .collect();
        keys.sort_unstable();
        let revoked_at: Option<String> = row.get(3);
        ensure!(
            keys == [
                "access",
                "agent",
                "created_at",
                "id",
                "revoked",
                "revoked_at"
            ] && item["created_at"] == row.get::<_, String>(2)
                && item["revoked_at"] == json!(revoked_at)
                && item["revoked"] == revoked_at.is_some(),
            "{item}"
        );
    }
    let find = |agent: &str| items.iter().find(|i| i["agent"] == agent);
    ensure!(
        find("planner-reader").is_some_and(|i| i["access"] == "read" && i["revoked"] == false)
            && find("bot").is_some_and(|i| i["access"] == "write" && i["revoked"] == true),
        "{listed}"
    );
    Ok(())
}
