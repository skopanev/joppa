// Part 6 of tests/node_workspace_http.rs, included there, so the test names stay unchanged.

// node-workspace-ac5
#[tokio::test]
async fn a_claim_locks_every_workspace_of_a_remote_chain() -> Result<()> {
    let w = world().await?;
    let q = chain_through_q(&w).await?;
    // A transaction holds the lock of Q, the second hop, as a record there does.
    let mut conn = w.pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&q],
    )
    .await?;
    let started = std::time::Instant::now();
    let claim = async {
        let result = w.claim().await;
        (result, started.elapsed())
    };
    let release = async {
        tokio::time::sleep(Duration::from_millis(600)).await;
        tx.commit().await
    };
    let ((claim, waited), released) = tokio::join!(claim, release);
    released?;
    claim?.map_err(|e| anyhow::anyhow!("claim after the lock: {e}"))?;
    ensure!(
        waited >= Duration::from_millis(550),
        "the claim must wait for the lock of Q: {waited:?}"
    );
    Ok(())
}

// node-workspace-ac5
#[tokio::test]
async fn a_parallel_claim_and_failing_record_two_hops_away_finish_in_one_order() -> Result<()> {
    let w = world().await?;
    let q = chain_through_q(&w).await?;
    let (claim, record) = tokio::join!(
        w.claim(),
        w.record_in(&q, "d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3", "fail")
    );
    record?;
    let claim = claim?;
    ensure!(
        claim.is_ok() || claim.as_ref().is_err_and(|e| e.contains(&w.waiting())),
        "the claim either ran first or saw the red second hop: {claim:?}"
    );
    let status = w.object(&w.bot, &w.p, "ac:a").await?["remote_links"][0]["status"].clone();
    ensure!(
        status["satisfied"] == false,
        "after both, CORE is red: {status}"
    );
    Ok(())
}

/// C publishes a new revision of API, X is green on it, and A repins to it.
async fn repin_api(w: &World) -> Result<()> {
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
    w.record("e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1", "pass")
        .await?;
    let pin = w.revision(&w.c, "requirement:api").await?;
    w.set_links(&w.bot, &w.p, "ac:a", json!([w.link(Some(pin), true)]))
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

// node-workspace-ac3
#[tokio::test]
async fn a_repin_after_a_claim_stops_the_old_plan_and_the_old_task() -> Result<()> {
    let w = world().await?;
    claimed_task(&w).await?;
    // Planning of A started on the old pin.
    w.claim()
        .await?
        .map_err(|e| anyhow::anyhow!("planning claim: {e}"))?;
    repin_api(&w).await?;
    let changed = "the remote links changed after the claim; release and claim again";
    match w
        .command(
            &w.bot,
            &w.p,
            json!({"op":"finish_planning","ac":"ac:a","reason":"The old plan"}),
        )
        .await?
    {
        Err(e) if e.contains(changed) => {}
        other => anyhow::bail!("the old plan must not finish: {other:?}"),
    }
    match complete(&w).await? {
        Err(e) if e.contains(changed) => {}
        other => anyhow::bail!("the old task must not complete: {other:?}"),
    }
    // A new claim after the review of the new pin can complete.
    let t = w.revision(&w.p, "task:t").await?;
    w.ok(
        &w.bot,
        &w.p,
        json!({"op":"release_task","id":"task:t","revision":t,"reason":"The pin changed"}),
    )
    .await?;
    let a = w.object(&w.owner, &w.p, "ac:a").await?;
    w.ok(&w.owner, &w.p, json!({"op":"assess","req":a["document"]["parent"],"revision":a["process"]["assessment"]["revision"],"ac":a["address"],
        "assessment":{"ready":true,"reason":"The new contract is reviewed"}})).await?;
    let t = w.revision(&w.p, "task:t").await?;
    w.ok(
        &w.bot,
        &w.p,
        json!({"op":"claim_task","id":"task:t","revision":t}),
    )
    .await?;
    complete(&w)
        .await?
        .map_err(|e| anyhow::anyhow!("completion after a new claim: {e}"))?;
    Ok(())
}
