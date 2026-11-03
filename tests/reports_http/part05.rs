// Part 5 of tests/reports_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn planning_claim_uses_the_current_batch_test_list_in_both_directions() -> Result<()> {
    let w = world().await?;
    w.list(&[A]).await?;
    ready_for_planning(&w).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "fail")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "the stored list must be green"
    );
    // A test node keeps the reset boundary of its criterion, so the earlier report counts.
    let w = &w;
    let batch = |tests: &'static [&'static str]| async move {
        let before = planning_criterion(w).await?;
        let claim = json!({"op":"claim_planning","ac":"ac:key-rights","revision":before["current_revision"]});
        let mut operations: Vec<Value> = bind_commands(&w.http, &w.base, &w.token, "ac:key-rights", tests)
            .await?
            .into_iter()
            .map(|command| json!({"command":command}))
            .collect();
        operations.push(json!({"command":claim}));
        planning_batch(w, json!(operations)).await
    };
    // The batch adds the failing test B: A turns red, so the claim in the same batch is valid.
    let reply = batch(&[A, B]).await?;
    ensure!(
        reply["result"]["isError"] != true && reply.get("error").is_none(),
        "the current red list must permit planning: {reply}"
    );
    ensure!(w.status().await?["status"] == "red", "the new list must be red");
    ensure!(
        planning_criterion(&w).await?["process"]["planning_claim"]["actor"] == "agent",
        "claim missing"
    );
    let (code, reply) = post(&w, &w.token, "/commands", json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{
        "op":"release_planning","ac":"ac:key-rights","reason":"Check the opposite batch transition"}})).await?;
    ensure!(code == 200, "{code} {reply}");
    // The batch archives B: A turns green in the batch, so the claim is refused and nothing changes.
    let reply = batch(&[A]).await?;
    ensure!(
        reply["result"]["isError"] == true
            && reply["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|e| e.ends_with("criterion is already verified")),
        "the current green list must refuse planning: {reply}"
    );
    ensure!(w.status().await?["status"] == "red", "the refused batch changed the list");
    ensure!(
        planning_criterion(&w).await?["process"]["planning_claim"].is_null(),
        "the refused batch kept a claim"
    );
    Ok(())
}

// status-current-only-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_listed_test_without_a_result_is_not_run_and_grey() -> Result<()> {
    // A lists opens (X) and refuses (Y); the only record of C with X passing.
    let w = world().await?;
    w.record("c1c1c1c", outcomes(&[("opens", "pass")])).await?;
    let s = w.status().await?;
    // An agent token reads the same grey tone.
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    ensure!(tone_at(&w.http, &w.base, bot, "ac:key-rights").await? == "grey");
    let test = |address: &str| {
        s["tests"]
            .as_array()
            .and_then(|tests| tests.iter().find(|t| t["address"] == address))
    };
    ensure!(
        test(A).is_some_and(|x| x["status"] == "pass" && x["commit"] == "c1c1c1c")
            && test(B).is_some_and(|y| y["status"] == "not_run")
            && s["status"] == "grey"
            && s["reason"] == "not all tests passed",
        "{s}"
    );
    Ok(())
}
