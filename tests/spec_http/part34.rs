// Part 34 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// A direct database change outside the journal is not a write, so the projection keeps the
/// status of the last write. When the projection is also stale, every read keeps the stored
/// status, and the catch-up after the start restores both the corrupted row and the projection.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_corrupted_criterion_row_and_a_stale_projection_recover_after_the_start() -> Result<()> {
    let w = world().await?;
    let node = "ac:key-rights";
    bind_tests(&w, &w.bot, node, &["checks.rs > key rights"]).await?;
    let passed = json!([{"test":"checks.rs > key rights","status":"pass"}]);
    record_http::recorded(&w.http, &w.root, &w.ws, &w.bot, "c4c4c4c", passed).await?;
    let passed = |object: &Value| object["signal"]["tone"] == "green";
    let before = w.get(&format!("address={node}")).await?;
    ensure!(passed(&before), "{before}");

    let row = "SELECT value::text FROM joppa.state_entries WHERE workspace=$1 AND section='objects' AND key='ac:key-rights'";
    let original: String = w.pool.get().await?.query_one(row, &[&w.ws]).await?.get(0);
    let broken = w.pool.get().await?.execute(
        "UPDATE joppa.state_entries SET value='{\"revisions\":\"unreadable\"}'::jsonb WHERE workspace=$1 AND section='objects' AND key='ac:key-rights'",
        &[&w.ws],
    ).await?;
    ensure!(broken == 1, "the criterion document must exist");
    // The corruption is not a journal write: the read keeps the status of the last write.
    let kept = w.get(&format!("address={node}")).await?;
    ensure!(passed(&kept), "{kept}");

    // A stale projection keeps the stored status on HTTP and on MCP.
    w.pool.get().await?.execute(
        "UPDATE joppa.board_cache SET first_view=first_view-'status_projection' WHERE workspace=$1",
        &[&w.ws],
    ).await?;
    let stale = w.get(&format!("address={node}")).await?;
    ensure!(passed(&stale), "{stale}");
    let mcp: Value = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.owner)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":w.ws,"address":node}}}))
        .send().await?.error_for_status()?.json().await?;
    ensure!(mcp["result"]["isError"] != true, "{mcp}");

    // The catch-up restores the row from the journal and rebuilds the projection.
    store::catch_up_statuses(&w.pool, false).await?;
    let restored: String = w.pool.get().await?.query_one(row, &[&w.ws]).await?.get(0);
    ensure!(
        restored == original,
        "restored {restored}, original {original}"
    );
    let after = w.get(&format!("address={node}")).await?;
    ensure!(passed(&after), "{after}");
    Ok(())
}
