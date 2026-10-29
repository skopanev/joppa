// Part 10 of tests/reports_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_saves_a_test_list_by_address() -> Result<()> {
    // checks-listed-first-ac1: skopanev (the owner token) saves one address.
    let w = world().await?;
    // A fresh criterion, so it starts with no saved test list.
    let command = json!({"op":"save_object","kind":"ac","id":"key-audit","revision":0,"parent":"requirement:key","data":criterion("Every key change is in the audit log"),"reason":"One more criterion"});
    let reply = w
        .http
        .post(format!("{}/commands", w.base))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
        .send()
        .await?;
    ensure!(
        reply.status().is_success(),
        "revise: {}",
        reply.text().await?
    );
    let read = || async {
        let s: Value = w
            .http
            .get(format!("{}/tests?ac=ac:key-audit", w.base))
            .bearer_auth(&w.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        anyhow::Ok(s)
    };
    // A fresh criterion has an empty list at its first document revision.
    let s = read().await?;
    ensure!(s["tests"] == json!([]) && s["tests_revision"] == 1, "{s}");
    let command = json!({"op":"set_ac_tests","target":"ac:key-audit","tests":["a.test.ts > login > ok"],"reason":"Link the test"});
    let reply = w
        .http
        .post(format!("{}/commands", w.base))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
        .send()
        .await?;
    ensure!(reply.status().is_success(), "{}", reply.text().await?);
    let s = read().await?;
    ensure!(
        s["tests"]
            .as_array()
            .map(|t| t.iter().map(|t| t["address"].clone()).collect::<Vec<_>>())
            == Some(vec![json!("a.test.ts > login > ok")])
            && s["tests_saved_by"] == "owner",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_cannot_save_a_test_address_without_a_file() -> Result<()> {
    // checks-listed-first-ac2: skopanev (the owner token) saves "login > ok" with no file.
    let w = world().await?;
    let before = w.status().await?;
    let command = json!({"op":"set_ac_tests","target":"ac:key-rights","tests":["login > ok"],"reason":"Link the test"});
    let (code, reply) = post(
        &w,
        &w.owner,
        "/commands",
        json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}),
    )
    .await?;
    ensure!(
        code == 409 && reply["error"] == "a test address needs file > name at least",
        "{code} {reply}"
    );
    let after = w.status().await?;
    ensure!(
        after["tests"] == before["tests"] && after["tests_saved_by"] == before["tests_saved_by"],
        "{before} {after}"
    );
    Ok(())
}

/// Opens passes, and refuses fails with "expected 403, got 200".
fn failing_refuses() -> Value {
    json!([{"test":A,"status":"pass"},{"test":B,"status":"fail","message":"expected 403, got 200"}])
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_test_shows_its_status_commit_and_message() -> Result<()> {
    // old-results-history-ac1: agent bot reads A over HTTP with its own token.
    let w = world().await?;
    w.record("c1c1c1c", failing_refuses()).await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let reply = w
        .http
        .get(format!("{}/tests?ac=ac:key-rights", w.base))
        .bearer_auth(bot)
        .send()
        .await?;
    let code = reply.status();
    let text = reply.text().await?;
    ensure!(code.is_success(), "{code} {text}");
    let s: Value = serde_json::from_str(&text)?;
    ensure!(
        s["tests"][0]["status"] == "pass"
            && s["tests"][0]["commit"] == "c1c1c1c"
            && s["tests"][1]["status"] == "fail"
            && s["tests"][1]["commit"] == "c1c1c1c"
            && s["tests"][1]["message"] == "expected 403, got 200",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn mcp_returns_each_test_with_status_and_commit() -> Result<()> {
    // old-results-history-ac3: agent bot calls the MCP tool joppa_tests with its own token.
    let w = world().await?;
    w.record("c1c1c1c", failing_refuses()).await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let workspace = w.base.rsplit('/').next().unwrap();
    let reply = mcp(
        &w,
        bot,
        "joppa_tests",
        json!({"workspace":workspace,"ac":"ac:key-rights"}),
    )
    .await?;
    let s = read_result("joppa_tests", &reply)?;
    ensure!(
        s["tests"][0]["status"] == "pass"
            && s["tests"][0]["commit"] == "c1c1c1c"
            && s["tests"][1]["status"] == "fail"
            && s["tests"][1]["commit"] == "c1c1c1c"
            && s["tests"][1]["message"] == "expected 403, got 200",
        "{reply}"
    );
    Ok(())
}

/// Workspace W with criteria A1 to A101, and the token that the owner created for agent bot.
async fn many_criteria() -> Result<(World, String, Vec<String>)> {
    let w = world().await?;
    for i in 1..=101 {
        let command = json!({"op":"save_object","kind":"ac","id":format!("a{i}"),"revision":0,"parent":"requirement:key","data":criterion(&format!("Criterion A{i} holds")),"reason":"Many criteria"});
        let (code, reply) = post(
            &w,
            &w.owner,
            "/commands",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
        )
        .await?;
        ensure!(code == 200, "criterion a{i}: {code} {reply}");
    }
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap().to_owned();
    let addresses = (1..=101).map(|i| format!("ac:a{i}")).collect();
    Ok((w, bot, addresses))
}

fn item_addresses(reply: &Value) -> Vec<&str> {
    reply["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["address"].as_str())
        .collect()
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn http_status_read_returns_all_101_criteria_in_request_order() -> Result<()> {
    // old-results-history-ac4: agent bot reads A1 to A101 in one HTTP request.
    let (w, bot, addresses) = many_criteria().await?;
    let reply = w
        .http
        .get(format!("{}/tests?ac={}", w.base, addresses.join(",")))
        .bearer_auth(&bot)
        .send()
        .await?;
    let code = reply.status().as_u16();
    let reply: Value = reply.json().await?;
    ensure!(code == 200, "{code} {reply}");
    ensure!(
        reply["items"].as_array().map(Vec::len) == Some(101),
        "item count: {reply}"
    );
    ensure!(item_addresses(&reply) == addresses, "{reply}");

    // Control: an unknown address at position 101 returns its error and no items.
    let mut listed = addresses[..100].to_vec();
    listed.push("ac:missing".into());
    let reply = w
        .http
        .get(format!("{}/tests?ac={}", w.base, listed.join(",")))
        .bearer_auth(&bot)
        .send()
        .await?;
    let code = reply.status().as_u16();
    let reply: Value = reply.json().await?;
    ensure!(
        code != 200
            && reply["items"].is_null()
            && reply["error"]
                .as_str()
                .is_some_and(|e| e.contains("unknown criterion ac:missing")),
        "{code} {reply}"
    );
    Ok(())
}
