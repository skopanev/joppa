// Part 14 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn search_finds_words_from_bodies_and_titles() -> Result<()> {
    // full-text-search-ac1, full-text-search-ac2
    let w = world().await?;
    w.note(
        "body-hit",
        0,
        "Unrelated title",
        "Users sign in with a passkey",
        json!([]),
    )
    .await?;
    w.note("title-hit", 0, "Passkey recovery", "Plain body", json!([]))
        .await?;
    let found = w.search("passkey").await?;
    ensure!(
        found.contains(&"note:body-hit".into()) && found.contains(&"note:title-hit".into()),
        "{found:?}"
    );
    Ok(())
}

// [ac:full-text-search-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot_finds_a_requirement_by_a_word_from_its_body() -> Result<()> {
    let w = world().await?;
    w.command(json!({"op":"revise","req":"sign-in","expected_revision":0,"capability":"restrict","title":"Sign in","body":"Users sign in with a passkey","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"sign-in-works","text":"Only members sign in"}]}))
        .await?;
    let result = mcp_call(
        &w,
        &w.bot,
        "joppa_search",
        json!({"workspace":w.ws,"query":"passkey"}),
    )
    .await?;
    ensure!(result["isError"] != true, "{result}");
    let found: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap_or("null"))?;
    let addresses: Vec<String> = found["results"]
        .as_array()
        .or(found["items"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|r| Some(format!("{}:{}", r["kind"].as_str()?, r["id"].as_str()?)))
        .collect();
    ensure!(
        addresses.contains(&"requirement:sign-in".into()),
        "{addresses:?} in {found}"
    );
    Ok(())
}

// [ac:full-text-search-ac2]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot_finds_a_requirement_by_a_word_from_its_title() -> Result<()> {
    let w = world().await?;
    w.command(json!({"op":"revise","req":"recovery","expected_revision":0,"capability":"restrict","title":"Recover a passkey","body":"Users regain access","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"recovery-works","text":"Only members recover access"}]}))
        .await?;
    let result = mcp_call(
        &w,
        &w.bot,
        "joppa_search",
        json!({"workspace":w.ws,"query":"passkey"}),
    )
    .await?;
    ensure!(result["isError"] != true, "{result}");
    let found: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap_or("null"))?;
    let addresses: Vec<String> = found["results"]
        .as_array()
        .or(found["items"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|r| Some(format!("{}:{}", r["kind"].as_str()?, r["id"].as_str()?)))
        .collect();
    ensure!(
        addresses.contains(&"requirement:recovery".into()),
        "{addresses:?} in {found}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn search_omits_archived_objects() -> Result<()> {
    // search-respects-access-ac1
    let w = world().await?;
    w.note("old", 0, "Legacy flow", "legacy", json!([])).await?;
    let (live, reply) = bot_search(&w, "legacy").await?;
    ensure!(live.contains(&"note:old".into()), "{reply}");
    let read = w.get("address=note:old").await?;
    w.command(json!({"op":"archive_object","target":"note:old","revision":read["document"]["revision"],"reason":"Cut"})).await?;
    let (found, reply) = bot_search(&w, "legacy").await?;
    ensure!(!found.contains(&"note:old".into()), "{reply}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn search_never_returns_another_workspaces_objects() -> Result<()> {
    // search-respects-access-ac2
    let a = world().await?;
    let b = world().await?;
    b.note(
        "secret",
        0,
        "Passkey plan",
        "passkey in workspace B only",
        json!([]),
    )
    .await?;
    let (control, reply) = bot_search(&b, "passkey").await?;
    ensure!(control.contains(&"note:secret".into()), "{reply}");
    let (found, reply) = bot_search(&a, "passkey").await?;
    ensure!(found.is_empty(), "{reply}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn another_workspace_answers_exactly_like_an_absent_one() -> Result<()> {
    // workspaces-isolated-ac1, invited-people-only-ac1: a stranger must not learn that B exists.
    let w = world().await?;
    let other = format!("http-spec-b-{}", uuid::Uuid::new_v4().simple());
    let create: Operation = serde_json::from_value(json!({"op":"workspace","title":"B"}))?;
    store::execute(&w.pool, &other, "someone-else", "create-b", &create).await?;
    let read = |ws: String| {
        let w = &w;
        async move {
            let reply = w
                .http
                .get(format!(
                    "{}/api/workspaces/{ws}/objects?limit=5&kind=domain",
                    w.root
                ))
                .bearer_auth(&w.owner)
                .send()
                .await?;
            Ok::<_, anyhow::Error>((reply.status().as_u16(), reply.text().await?))
        }
    };
    let existing = read(other).await?;
    let absent = read(format!("absent-{}", uuid::Uuid::new_v4().simple())).await?;
    ensure!(
        existing.0 == 404 && existing == absent,
        "{existing:?} vs {absent:?}"
    );
    ensure!(existing.1.contains("Workspace not found"), "{existing:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_account_without_an_invite_cannot_read_an_object_over_http() -> Result<()> {
    // [ac:invited-people-only-ac1] eve reads an object and gets the absent-workspace reply.
    let w = world().await?;
    let id = uuid::Uuid::new_v4().simple();
    let title = format!("Private title {id}");
    let body = format!("Private body {id}");
    w.note("eve-target", 0, &title, &body, json!([])).await?;
    let read = |ws: String, token: String| {
        let w = &w;
        async move {
            let reply = w
                .http
                .get(format!(
                    "{}/api/workspaces/{ws}/objects?address=note:eve-target",
                    w.root
                ))
                .bearer_auth(token)
                .send()
                .await?;
            Ok::<_, anyhow::Error>((reply.status().as_u16(), reply.text().await?))
        }
    };
    let owner = read(w.ws.clone(), w.owner.clone()).await?;
    let saved: Value = serde_json::from_str(&owner.1)?;
    ensure!(
        owner.0 == 200 && saved["document"]["data"]["title"] == title.as_str(),
        "{owner:?}"
    );
    let member_only = read(w.ws.clone(), w.eve.clone()).await?;
    let absent = read(
        format!("absent-{}", uuid::Uuid::new_v4().simple()),
        w.eve.clone(),
    )
    .await?;
    for reply in [&member_only, &absent] {
        ensure!(
            reply.0 == 404 && reply.1 == r#"{"error":"Workspace not found"}"#,
            "{member_only:?} vs {absent:?}"
        );
        ensure!(
            !reply.1.contains(&title) && !reply.1.contains(&body),
            "{member_only:?} vs {absent:?}"
        );
    }
    ensure!(member_only == absent, "{member_only:?} vs {absent:?}");
    Ok(())
}
