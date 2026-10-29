// Part 37 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// Bot search through the HTTP route GET /api/workspaces/{ws}/search.
/// Returns the result addresses and the reply.
async fn bot_http_search(w: &World, query: &str) -> Result<(Vec<String>, Value)> {
    let reply = w
        .http
        .get(format!("{}/api/workspaces/{}/search", w.root, w.ws))
        .query(&[("q", query)])
        .bearer_auth(&w.bot)
        .send()
        .await?;
    let status = reply.status();
    let found: Value = reply.json().await?;
    ensure!(status == 200, "search {query}: {status} {found}");
    let rows = found["items"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("no result array: {found}"))?;
    let addresses = rows
        .iter()
        .map(|r| match (r["kind"].as_str(), r["id"].as_str()) {
            (Some(kind), Some(id)) => Ok(format!("{kind}:{id}")),
            _ => anyhow::bail!("result without an address: {found}"),
        })
        .collect::<Result<_>>()?;
    Ok((addresses, found))
}

// [ac:full-text-search-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot_finds_a_requirement_by_a_word_from_its_body_over_http() -> Result<()> {
    let w = world().await?;
    w.requirement("sign-in", "restrict", "Sign in", "Users sign in with a passkey", &[("sign-in-works", "Only members sign in")])
        .await?;
    let (found, reply) = bot_http_search(&w, "passkey").await?;
    ensure!(found.contains(&"requirement:sign-in".into()), "{reply}");
    Ok(())
}

// [ac:full-text-search-ac2]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot_finds_a_requirement_by_a_word_from_its_title_over_http() -> Result<()> {
    let w = world().await?;
    w.requirement("recovery", "restrict", "Recover a passkey", "Users regain access", &[("recovery-works", "Only members recover access")])
        .await?;
    let (found, reply) = bot_http_search(&w, "passkey").await?;
    ensure!(found.contains(&"requirement:recovery".into()), "{reply}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn search_over_http_omits_archived_objects() -> Result<()> {
    // search-respects-access-ac1: the live object is found first, as a control.
    let w = world().await?;
    w.note("old", 0, "Legacy flow", "legacy", json!([])).await?;
    let (live, reply) = bot_http_search(&w, "legacy").await?;
    ensure!(live.contains(&"note:old".into()), "{reply}");
    let read = w.get("address=note:old").await?;
    w.command(json!({"op":"archive_object","target":"note:old","revision":read["document"]["revision"],"reason":"Cut"})).await?;
    let (found, reply) = bot_http_search(&w, "legacy").await?;
    ensure!(!found.contains(&"note:old".into()), "{reply}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn search_over_http_never_returns_another_workspaces_objects() -> Result<()> {
    // search-respects-access-ac2: the word "passkey" exists only in workspace B.
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
    let (control, reply) = bot_http_search(&b, "passkey").await?;
    ensure!(control.contains(&"note:secret".into()), "{reply}");
    let (found, reply) = bot_http_search(&a, "passkey").await?;
    ensure!(found.is_empty() && reply["items"] == json!([]), "{reply}");
    Ok(())
}
