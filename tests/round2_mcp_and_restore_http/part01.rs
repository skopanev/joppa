// Part 1 of tests/round2_mcp_and_restore_http.rs: ac:dsj99sz6 and ac:yyfmvx37.

// ac:dsj99sz6 (nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn record_results_schema_requires_only_workspace_and_results_and_accepts_commitless_record()
-> Result<()> {
    let w = record_world::world(&["suite > check_one"]).await?;
    let tools = tools_list(&w).await?;
    let tool = tools
        .iter()
        .find(|t| t["name"] == "joppa_record_results")
        .context("missing joppa_record_results")?;
    ensure!(
        tool["inputSchema"]["required"] == json!(["workspace", "results"]),
        "required: {}",
        tool["inputSchema"]["required"]
    );
    ensure!(
        tool["inputSchema"]["properties"]["commit"].is_object(),
        "commit property missing: {tool}"
    );

    let tree = mcp_ok(&w, "joppa_tree", json!({})).await?;
    let mut tr = tree["schema"]["test_results"].clone();
    tr["commit_required"] = json!(false);
    w.command(
        &w.owner,
        json!({"op": "set_tree_schema", "revision": tree["schema"]["revision"], "test_results": tr, "reason": "Allow commitless records"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    let recorded = mcp_ok(
        &w,
        "joppa_record_results",
        json!({"results": [{"test": "suite > check_one", "status": "pass"}]}),
    )
    .await?;
    ensure!(recorded["unbound"] == json!([]), "{recorded}");
    ensure!(w.object("ac:a").await?["status"]["tone"] == "green");
    Ok(())
}

// ac:dsj99sz6 (off-nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn record_results_without_commit_stays_refused_under_default_commit_required() -> Result<()> {
    let w = record_world::world(&["suite > check_one"]).await?;
    let before = head(&w).await?;
    let reply = w
        .mcp(
            &w.bot,
            "joppa_record_results",
            json!({"results": [{"test": "suite > check_one", "status": "pass"}]}),
        )
        .await?;
    refused(reply, "commit must be a lowercase git sha")?;
    ensure!(head(&w).await? == before);
    Ok(())
}

// ac:yyfmvx37 (nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn known_data_filter_succeeds_and_dedicated_removed_argument_errors_stay_intact() -> Result<()>
{
    let w = record_world::world(&[]).await?;
    ok(
        &w,
        json!({"op": "save_object", "kind": "ac", "id": "a", "revision": 1, "data": {"tags": ["x"]}, "reason": "Tag a"}),
    )
    .await?;
    let counted = mcp_ok(
        &w,
        "joppa_objects",
        json!({"kind": "ac", "parent": "*", "count": true, "data": {"tags": ["x"]}}),
    )
    .await?;
    ensure!(counted["total"] == 1, "{counted}");

    refused(
        w.mcp(&w.bot, "joppa_objects", json!({"work_filter": "ready"}))
            .await?,
        "work_filter was removed",
    )?;
    refused(
        w.mcp(&w.bot, "joppa_objects", json!({"ready": true}))
            .await?,
        "the ready filter was removed",
    )?;
    refused(
        w.mcp(
            &w.bot,
            "joppa_command",
            json!({"actor": "spoof", "request_id": "r1", "command": {"op": "note", "target": "ac:a", "id": "c1", "text": "hi"}}),
        )
        .await?,
        "actor comes from the identity, not the request",
    )?;
    Ok(())
}

// ac:yyfmvx37 (off-nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn tools_call_refuses_unknown_top_level_arguments_across_mcp_tools() -> Result<()> {
    let w = record_world::world(&["suite > check_one"]).await?;
    for (tool, args, expected) in [
        (
            "joppa_objects",
            json!({"kind": "ac", "count": true, "tags": "x"}),
            "unknown argument `tags`",
        ),
        (
            "joppa_history",
            json!({"foo": 1}),
            "unknown argument `foo`",
        ),
        (
            "joppa_record_results",
            json!({"commit": "abcdef1", "results": [{"test": "suite > check_one", "status": "pass"}], "branch": "main"}),
            "unknown argument `branch`",
        ),
        (
            "joppa_search",
            json!({"query": "A", "filter": {}}),
            "unknown argument `filter`",
        ),
    ] {
        refused(w.mcp(&w.bot, tool, args).await?, expected)?;
    }
    Ok(())
}
