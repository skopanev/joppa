// Part 5 of tests/node_workspace_http.rs, included there, so the test names stay unchanged.

// node-workspace-ac6
#[tokio::test]
async fn search_timeline_and_tasks_stay_in_their_own_workspace() -> Result<()> {
    let w = world().await?;
    w.ok(
        &w.owner,
        &w.p,
        World::save(
            "component",
            "backend",
            0,
            None,
            json!({"title":"Backend","owner":"owner","workspace":w.c}),
        ),
    )
    .await?;
    let found = w
        .mcp(
            &w.owner,
            "joppa_search",
            json!({"workspace":w.p,"query":"contract"}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    ensure!(
        found["items"].as_array().is_some_and(Vec::is_empty),
        "P search finds no C object: {found}"
    );
    let found = w
        .mcp(
            &w.owner,
            "joppa_search",
            json!({"workspace":w.c,"query":"contract"}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let kinds: Vec<&str> = found["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["address"].as_str())
        .collect();
    ensure!(
        kinds.contains(&"requirement:api"),
        "C search finds its own object: {found}"
    );
    let timeline = w
        .mcp(
            &w.owner,
            "joppa_timeline",
            json!({"workspace":w.p,"limit":100}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let addresses: Vec<&str> = timeline["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["address"].as_str())
        .collect();
    ensure!(
        !addresses
            .iter()
            .any(|a| *a == "requirement:api" || *a == "ac:x"),
        "{addresses:?}"
    );
    let tasks = w
        .mcp(
            &w.owner,
            "joppa_objects",
            json!({"workspace":w.p,"parent":"*","limit":100}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let listed: Vec<&str> = tasks["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["address"].as_str())
        .collect();
    ensure!(
        listed.contains(&"component:backend") && !listed.contains(&"requirement:api"),
        "{listed:?}"
    );
    Ok(())
}

/// P: task T under A, claimed by bot while API is green.
async fn claimed_task(w: &World) -> Result<()> {
    w.block_a_on_api().await?;
    w.record("b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1", "pass")
        .await?;
    w.ok(&w.owner, &w.p, json!({"op":"save_object","kind":"task","id":"t","revision":0,"parent":"ac:a",
        "data":{"title":"Build login","owner":"owner","contract":"Call the API","purpose":"implementation","projects":[]},"reason":"Fixture"})).await?;
    let t = w.revision(&w.p, "task:t").await?;
    w.ok(
        &w.bot,
        &w.p,
        json!({"op":"claim_task","id":"task:t","revision":t}),
    )
    .await?;
    Ok(())
}

async fn complete(w: &World) -> Result<Result<Value, String>> {
    let t = w.revision(&w.p, "task:t").await?;
    w.command(
        &w.bot,
        &w.p,
        json!({"op":"complete_task","commit":"c0ffee0","id":"task:t","revision":t,"evidence":"Called the API"}),
    )
    .await
}

// node-workspace-ac5
#[tokio::test]
async fn completion_and_acceptance_recheck_a_remote_target_under_the_locks() -> Result<()> {
    let w = world().await?;
    claimed_task(&w).await?;
    // API turns red after the claim: the holder cannot complete until it is green again.
    w.record("b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2", "fail")
        .await?;
    match complete(&w).await? {
        Err(e) if e.contains(&w.waiting()) => {}
        other => anyhow::bail!("completion must wait for API: {other:?}"),
    }
    w.record("b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3", "pass")
        .await?;
    complete(&w)
        .await?
        .map_err(|e| anyhow::anyhow!("completion after green: {e}"))?;
    // R passes its own tests, but its acceptance waits while API is red.
    w.ok(
        &w.owner,
        &w.p,
        json!({"op":"set_ac_tests","target":"ac:a","tests":["tests/a.rs > a"],"reason":"Bind"}),
    )
    .await?;
    let commit = "c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1";
    w.mcp(&w.bot, "joppa_record_results", json!({"workspace":w.p,"commit":commit,"results":[{"test":"tests/a.rs > a","status":"pass"}]}))
        .await?.map_err(|e| anyhow::anyhow!("{e}"))?;
    w.record("b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4", "fail")
        .await?;
    let accept = async || {
        let r = w.revision(&w.p, "requirement:r").await?;
        let req = w.object(&w.owner, &w.p, "requirement:r").await?["revision"].clone();
        w.command(
            &w.owner,
            &w.p,
            json!({"op":"accept_commit","target":"requirement:r","revision":req,"commit":commit}),
        )
        .await
        .map(|reply| (r, reply))
    };
    match accept().await?.1 {
        Err(e) if e.contains(&w.waiting()) => {}
        other => anyhow::bail!("acceptance must wait for API: {other:?}"),
    }
    w.record("b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5", "pass")
        .await?;
    accept()
        .await?
        .1
        .map_err(|e| anyhow::anyhow!("acceptance after green: {e}"))?;
    Ok(())
}

// node-workspace-ac5
#[tokio::test]
async fn a_parallel_completion_and_failing_record_finish_in_one_consistent_order() -> Result<()> {
    let w = world().await?;
    claimed_task(&w).await?;
    let (done, record) = tokio::join!(
        complete(&w),
        w.record("b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6", "fail")
    );
    record?;
    let done = done?;
    ensure!(
        done.is_ok() || done.as_ref().is_err_and(|e| e.contains(&w.waiting())),
        "the completion either ran first or saw the red target: {done:?}"
    );
    Ok(())
}

/// Q is a third workspace: P:A depends on C:API, and C:API depends on Q:CORE. Both green.
async fn chain_through_q(w: &World) -> Result<String> {
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
    w.record_in(&w.c, "d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1", "pass")
        .await?;
    w.record_in(&q, "d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2", "pass")
        .await?;
    Ok(q)
}
