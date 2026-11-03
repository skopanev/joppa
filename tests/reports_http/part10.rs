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
    let read = || store::test_status(&w.pool, w.root_ws().1, "agent", "ac:key-audit");
    // A fresh criterion has no test node.
    let s = read().await?;
    ensure!(s["tests"] == json!([]), "{s}");
    bind_at(
        &w.http,
        &w.base,
        &w.owner,
        "ac:key-audit",
        &["a.test.ts > login > ok"],
    )
    .await?;
    let s = read().await?;
    ensure!(listed(&s) == ["a.test.ts > login > ok"], "{s}");
    let nodes = test_nodes(&w.http, &w.base, &w.owner, "ac:key-audit").await?;
    let address = nodes[0]["address"].as_str().unwrap_or_default();
    let node: Value = w
        .http
        .get(format!("{}/objects?address={address}", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    ensure!(node["document"]["actor"] == "owner", "{node}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_cannot_save_a_test_address_without_a_file() -> Result<()> {
    // checks-listed-first-ac2: skopanev (the owner token) saves "login > ok" with no file.
    let w = world().await?;
    let before = w.status().await?;
    let command = json!({"op":"save_object","kind":"test","revision":0,"parent":"ac:key-rights","data":{"title":"login > ok","owner":"owner"},"reason":"Link the test"});
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
    ensure!(after == before, "{before} {after}");
    Ok(())
}

/// Opens passes, and refuses fails with "expected 403, got 200".
fn failing_refuses() -> Value {
    json!([{"test":A,"status":"pass"},{"test":B,"status":"fail","message":"expected 403, got 200"}])
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_test_shows_its_status_commit_and_message() -> Result<()> {
    // old-results-history-ac1: agent bot reads A with its own token.
    let w = world().await?;
    w.record("c1c1c1c", failing_refuses()).await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    // The bot reads the tone of each test node over HTTP, and the status names commit and message.
    let mut tones: Vec<(Value, Value)> = test_nodes(&w.http, &w.base, bot, "ac:key-rights")
        .await?
        .iter()
        .map(|n| (n["title"].clone(), n["signal"]["tone"].clone()))
        .collect();
    tones.sort_by_key(|(title, _)| title.to_string());
    ensure!(
        tones == [(json!(A), json!("green")), (json!(B), json!("red"))],
        "{tones:?}"
    );
    let s = store::test_status(&w.pool, w.root_ws().1, "bot", "ac:key-rights").await?;
    ensure!(
        test_of(&s, A)["status"] == "pass"
            && test_of(&s, A)["commit"] == "c1c1c1c"
            && test_of(&s, B)["status"] == "fail"
            && test_of(&s, B)["commit"] == "c1c1c1c"
            && test_of(&s, B)["message"] == "expected 403, got 200",
        "{s}"
    );
    Ok(())
}
