// Part 4 of tests/node_workspace_http.rs, included there, so the test names stay unchanged.

// node-workspace-ac3
#[tokio::test]
async fn a_green_target_with_a_newer_revision_does_not_free_work_until_a_new_pin() -> Result<()> {
    let w = world().await?;
    w.block_a_on_api().await?;
    w.record("6666666666666666666666666666666666666666", "pass")
        .await?;
    let pin = w.revision(&w.c, "requirement:api").await?;
    w.ok(
        &w.owner,
        &w.c,
        World::save(
            "requirement",
            "api",
            pin,
            None,
            json!({"title":"API","owner":"owner","body":"The API contract, version 2"}),
        ),
    )
    .await?;
    // The text change of API makes X grey until a new record, so record again: X is green.
    w.record("7777777777777777777777777777777777777777", "pass")
        .await?;
    let read = w.object(&w.bot, &w.p, "ac:a").await?;
    ensure!(
        read["remote_links"][0]["status"]["review_required"] == true
            && read["remote_links"][0]["status"]["satisfied"] == false,
        "{read}"
    );
    match w.claim().await? {
        Err(e) if e.contains(&w.waiting()) => {}
        other => anyhow::bail!("a stale pin must not free work: {other:?}"),
    }
    w.block_a_on_api().await?;
    w.claim()
        .await?
        .map_err(|e| anyhow::anyhow!("claim after the new pin: {e}"))?;
    Ok(())
}

// node-workspace-ac3
#[tokio::test]
async fn a_red_dependency_of_a_green_target_keeps_work_waiting() -> Result<()> {
    let w = world().await?;
    // Q is a third workspace. API in C depends on CORE in Q; A in P depends on API.
    let q = workspace(&w.pool, "core", false).await?;
    w.ok(
        &w.owner,
        &q,
        World::save(
            "requirement",
            "core",
            0,
            None,
            json!({"title":"Core","owner":"owner","body":"The core rule"}),
        ),
    )
    .await?;
    w.ok(
        &w.owner,
        &q,
        World::save(
            "ac",
            "y",
            0,
            Some("requirement:core"),
            json!({"title":"Y","owner":"owner"}),
        ),
    )
    .await?;
    w.ok(
        &w.owner,
        &q,
        json!({"op":"set_ac_tests","target":"ac:y","tests":[TEST],"reason":"Bind"}),
    )
    .await?;
    let core = w.revision(&q, "requirement:core").await?;
    let to_core = json!([{"workspace":q,"target":"requirement:core","relation":"depends_on","revision":core,"blocking":true}]);
    w.set_links(&w.owner, &w.c, "requirement:api", to_core)
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    w.block_a_on_api().await?;

    w.record_in(&w.c, "8888888888888888888888888888888888888888", "pass")
        .await?;
    w.record_in(&q, "9999999999999999999999999999999999999999", "fail")
        .await?;
    // X in C is green, but CORE in Q is red, so API is not ready and A waits.
    match w.claim().await? {
        Err(e) if e.contains(&w.waiting()) => {}
        other => anyhow::bail!("a red transitive dependency must hold A: {other:?}"),
    }
    w.record_in(&q, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "pass")
        .await?;
    w.claim()
        .await?
        .map_err(|e| anyhow::anyhow!("claim after CORE is green: {e}"))?;
    Ok(())
}

// node-workspace-ac4
#[tokio::test]
async fn a_blocking_cycle_across_workspaces_is_refused() -> Result<()> {
    let w = world().await?;
    w.block_a_on_api().await?;
    let a = w.revision(&w.p, "ac:a").await?;
    let back = json!([{"workspace":w.p,"target":"ac:a","relation":"depends_on","revision":a,"blocking":true}]);
    match w.set_links(&w.bot, &w.c, "requirement:api", back).await? {
        Err(e) if e.contains("dependency cycle across workspaces") => Ok(()),
        other => anyhow::bail!("expected the cycle refusal, got {other:?}"),
    }
}

// node-workspace-ac5
#[tokio::test]
async fn a_claim_waits_for_a_result_write_in_the_target_workspace() -> Result<()> {
    let w = world().await?;
    w.block_a_on_api().await?;
    w.record("3333333333333333333333333333333333333333", "pass")
        .await?;
    // A transaction holds the lock of C, as a record of results does while it writes.
    let mut conn = w.pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&w.c],
    )
    .await?;
    let claim = tokio::spawn({
        let (root, p, bot) = (w.root.clone(), w.p.clone(), w.bot.clone());
        async move {
            let http = reqwest::Client::new();
            let read: Value = http.post(format!("{root}/mcp")).bearer_auth(&bot)
                .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":p,"address":"ac:a"}}}))
                .send().await?.json().await?;
            let a: Value = serde_json::from_str(
                read["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or("null"),
            )?;
            let reply: Value = http.post(format!("{root}/mcp")).bearer_auth(&bot)
                .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":p,"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"claim_planning","ac":"ac:a","revision":a["revision"]}}}}))
                .send().await?.json().await?;
            anyhow::Ok(reply)
        }
    });
    tokio::time::sleep(Duration::from_millis(600)).await;
    ensure!(
        !claim.is_finished(),
        "the claim must wait for the lock of C"
    );
    tx.commit().await?;
    let reply = claim.await??;
    ensure!(
        reply["result"]["isError"] != true,
        "the claim finishes after the lock: {reply}"
    );
    Ok(())
}

// node-workspace-ac5
#[tokio::test]
async fn a_parallel_claim_and_failing_record_finish_in_one_consistent_order() -> Result<()> {
    let w = world().await?;
    w.block_a_on_api().await?;
    w.record("4444444444444444444444444444444444444444", "pass")
        .await?;
    let (claim, record) = tokio::join!(
        w.claim(),
        w.record("5555555555555555555555555555555555555555", "fail")
    );
    record?;
    let claim = claim?;
    ensure!(
        claim.is_ok() || claim.as_ref().is_err_and(|e| e.contains(&w.waiting())),
        "the claim either ran first or saw the red target: {claim:?}"
    );
    // After both, the target is red, so a new claim waits.
    let status = w.object(&w.bot, &w.p, "ac:a").await?["remote_links"][0]["status"].clone();
    ensure!(status["satisfied"] == false, "{status}");
    Ok(())
}
