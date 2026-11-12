// Part 4 of tests/node_workspace_http.rs, included there, so the test names stay unchanged.

// node-workspace-ac3
#[tokio::test]
async fn a_newer_revision_of_a_green_target_asks_for_review_until_a_new_pin() -> Result<()> {
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
        read["remote_links"][0]["status"]["review_required"] == true,
        "{read}"
    );
    // A new pin ends the review.
    w.block_a_on_api().await?;
    let read = w.object(&w.bot, &w.p, "ac:a").await?;
    ensure!(
        read["remote_links"][0]["status"]["review_required"] == false,
        "{read}"
    );
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
