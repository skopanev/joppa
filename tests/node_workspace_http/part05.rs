// Part 5 of tests/node_workspace_http.rs, included there, so the test names stay unchanged.

// node-workspace-ac6
#[tokio::test]
async fn search_history_and_tasks_stay_in_their_own_workspace() -> Result<()> {
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
    let history = w
        .mcp(
            &w.owner,
            "joppa_history",
            json!({"workspace":w.p,"limit":100}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let addresses: Vec<&str> = history["items"]
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

// node-workspace-ac5
#[tokio::test]
async fn acceptance_rechecks_a_remote_target_under_the_locks() -> Result<()> {
    let w = world().await?;
    w.block_a_on_api().await?;
    w.record("b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1", "pass")
        .await?;
    // R passes its own tests, but its acceptance waits while API is red.
    w.ok(
        &w.owner,
        &w.p,
        World::bind("ac:a", "tests/a.rs > a"),
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
