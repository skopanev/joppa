// Part 3 of tests/node_workspace_http.rs, included there, so the test names stay unchanged.

// node-workspace-ac1
#[tokio::test]
async fn a_node_shows_the_child_workspace_status_with_the_access_of_the_reader() -> Result<()> {
    let w = world().await?;
    let node = |ws: &str| json!({"title":"Backend","owner":"owner","workspace":ws});
    w.ok(
        &w.owner,
        &w.p,
        World::save("component", "backend", 0, None, node(&w.c)),
    )
    .await?;
    let read = w.object(&w.owner, &w.p, "component:backend").await?;
    ensure!(
        read["child_workspace"]
            == json!({"workspace":w.c,"access":true,"status":"grey","criteria":{"green":0,"red":0,"total":1}}),
        "{read}"
    );
    w.record("1111111111111111111111111111111111111111", "pass")
        .await?;
    let read = w.object(&w.owner, &w.p, "component:backend").await?;
    ensure!(read["child_workspace"]["status"] == "green", "{read}");
    // A workspace name is stored as the immutable ID, and reads name that ID.
    let alias = format!("renamed-{}", uuid::Uuid::new_v4().simple());
    w.ok(
        &w.owner,
        &w.c,
        json!({"op":"rename_workspace","workspace_id":alias,"title":"Backend","reason":"New name"}),
    )
    .await?;
    w.ok(
        &w.owner,
        &w.p,
        World::save("component", "mobile", 0, None, node(&alias)),
    )
    .await?;
    let mobile = w.object(&w.owner, &w.p, "component:mobile").await?;
    ensure!(
        mobile["data"]["workspace"] == json!(w.c)
            && mobile["child_workspace"]["workspace"] == json!(w.c),
        "{mobile}"
    );
    // A detail read by the key gives no workspace key; joppa_tree gives it.
    let api = w.object(&w.owner, &w.c, "requirement:api").await?;
    ensure!(
        api.get("workspace_key").is_none() && api["data"].is_object(),
        "{api}"
    );
    let tree = w
        .mcp(&w.owner, "joppa_tree", json!({"workspace":alias}))
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    // A read by an alias names the workspace key, and not the alias that the request sent.
    ensure!(
        tree["workspace_key"] == json!(w.c) && tree.get("workspace").is_none(),
        "{tree}"
    );
    let alex = w.object(&w.alex, &w.p, "component:backend").await?;
    ensure!(
        alex["child_workspace"] == json!({"workspace":w.c,"access":false,"status":"grey"}),
        "a reader without access sees no data: {alex}"
    );
    // The own workspace, an unknown workspace and a workspace without access are refused.
    w.refused(
        &w.owner,
        &w.p,
        World::save("component", "own", 0, None, node(&w.p)),
        "names another workspace",
    )
    .await?;
    w.refused(
        &w.owner,
        &w.p,
        World::save("component", "none", 0, None, node("missing-workspace")),
        "no read access to workspace missing-workspace",
    )
    .await?;
    w.refused(
        &w.alex,
        &w.p,
        World::save(
            "component",
            "alex",
            0,
            None,
            json!({"title":"Backend","owner":"alex","workspace":w.c}),
        ),
        &format!("no read access to workspace {}", w.c),
    )
    .await?;
    Ok(())
}

// node-workspace-ac2
#[tokio::test]
async fn a_remote_link_checks_its_target_and_shows_a_changed_contract() -> Result<()> {
    let w = world().await?;
    let pin = w.revision(&w.c, "requirement:api").await?;
    w.set_links(&w.bot, &w.p, "ac:a", json!([w.link(Some(pin), false)]))
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let read = w.object(&w.bot, &w.p, "ac:a").await?;
    ensure!(
        read["process"]["assessment"]["state"] == "unassessed",
        "a new pin resets the criteria below: {read}"
    );
    ensure!(
        read["remote_links_revision"] == 1 && read["remote_review_required"] == false,
        "{read}"
    );
    let status = &read["remote_links"][0]["status"];
    ensure!(
        *status
            == json!({"access":true,"present":true,"current_revision":pin,"contract_revision":pin,"review_required":false,"satisfied":false}),
        "{read}"
    );
    // C changes its contract: the pinned link now asks for a review.
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
    let read = w.object(&w.bot, &w.p, "ac:a").await?;
    ensure!(
        read["remote_links"][0]["status"]["review_required"] == true
            && read["remote_review_required"] == true,
        "{read}"
    );
    let alex = w.object(&w.alex, &w.p, "ac:a").await?;
    ensure!(
        alex["remote_links"][0]["status"]
            == json!({"access":false,"present":null,"current_revision":null,"review_required":null,"satisfied":false}),
        "{alex}"
    );
    // A stale links revision, a future pin, an unknown target, the own workspace and no access.
    let stale =
        json!({"op":"set_remote_links","target":"ac:a","revision":0,"links":[],"reason":"Stale"});
    w.refused(&w.bot, &w.p, stale, "stale remote links revision")
        .await?;
    let mut bad = w.link(Some(pin + 9), false);
    let refuse =
        async |links: Value, error: &str| match w.set_links(&w.bot, &w.p, "ac:a", links).await? {
            Err(e) if e.contains(error) => anyhow::Ok(()),
            other => anyhow::bail!("expected {error:?}, got {other:?}"),
        };
    refuse(json!([bad.clone()]), "unknown linked revision").await?;
    bad["revision"] = json!(1);
    bad["target"] = json!("requirement:none");
    refuse(json!([bad.clone()]), "unknown or archived object").await?;
    bad["target"] = json!("requirement:r");
    bad["workspace"] = json!(w.p);
    refuse(json!([bad]), "names another workspace").await?;
    match w
        .set_links(&w.alex, &w.p, "ac:a", json!([w.link(Some(1), false)]))
        .await?
    {
        Err(e) if e.contains(&format!("no read access to workspace {}", w.c)) => {}
        other => anyhow::bail!("alex: {other:?}"),
    }
    Ok(())
}

// node-workspace-ac2
#[tokio::test]
async fn an_archived_target_is_shown_for_review_and_a_batch_writes_links_atomically() -> Result<()>
{
    let w = world().await?;
    let pin = w.revision(&w.c, "ac:x").await?;
    // One batch saves a note and its remote link; a refused link rolls back the note.
    let batch = |id: &str, target: &str| {
        json!({"workspace":w.p,"request_id":uuid::Uuid::new_v4().to_string(),"operations":[
        {"command":{"op":"save_object","kind":"requirement","id":id,"revision":0,"parent":null,"data":{"title":"Mobile","owner":"owner","body":"Uses the API"},"reason":"Fixture"}},
        {"command":{"op":"set_remote_links","target":format!("requirement:{id}"),"revision":0,"links":[{"workspace":w.c,"target":target,"relation":"related","revision":pin}],"reason":"Mobile uses X"}}]})
    };
    w.mcp(&w.bot, "joppa_batch", batch("mobile", "ac:x"))
        .await?
        .map_err(|e| anyhow::anyhow!("batch: {e}"))?;
    ensure!(
        w.mcp(&w.bot, "joppa_batch", batch("broken", "ac:none"))
            .await?
            .is_err(),
        "a bad link refuses the batch"
    );
    ensure!(
        w.mcp(
            &w.bot,
            "joppa_objects",
            json!({"workspace":w.p,"address":"requirement:broken"})
        )
        .await?
        .is_err(),
        "the refused batch saved nothing"
    );
    w.ok(
        &w.owner,
        &w.c,
        json!({"op":"archive_object","target":"ac:x","revision":pin,"reason":"X is retired"}),
    )
    .await?;
    let read = w.object(&w.bot, &w.p, "requirement:mobile").await?;
    ensure!(
        read["remote_review_required"] == true
            && read["remote_links"][0]["status"]
                == json!({"access":true,"present":false,"current_revision":null,"review_required":true,"satisfied":false}),
        "{read}"
    );
    Ok(())
}

// node-workspace-ac3
#[tokio::test]
async fn work_waits_for_a_blocking_remote_link_until_its_target_is_green() -> Result<()> {
    let w = world().await?;
    w.block_a_on_api().await?;
    match w.claim().await? {
        Err(e) if e.contains(&w.waiting()) => {}
        other => anyhow::bail!("expected the wait, got {other:?}"),
    }
    w.record("2222222222222222222222222222222222222222", "pass")
        .await?;
    w.claim()
        .await?
        .map_err(|e| anyhow::anyhow!("claim after green: {e}"))?;
    Ok(())
}
