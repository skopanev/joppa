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
    // The text change of API makes X blue until a new record, so record again: X is green.
    w.record("7777777777777777777777777777777777777777", "pass")
        .await?;
    let read = w.object(&w.bot, &w.p, "ac:a").await?;
    ensure!(
        read["remote_links"][0]["status"]["review_required"] == true
            && read["remote_links"][0]["status"]["satisfied"] == false,
        "{read}"
    );
    match w.free().await? {
        Err(e) if e.contains(&w.waiting()) => {}
        other => anyhow::bail!("a stale pin must not free work: {other:?}"),
    }
    w.block_a_on_api().await?;
    w.free()
        .await?
        .map_err(|e| anyhow::anyhow!("free after the new pin: {e}"))?;
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
        World::bind("ac:y", TEST),
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
    match w.free().await? {
        Err(e) if e.contains(&w.waiting()) => {}
        other => anyhow::bail!("a red transitive dependency must hold A: {other:?}"),
    }
    w.record_in(&q, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "pass")
        .await?;
    w.free()
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
