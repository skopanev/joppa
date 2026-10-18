// Part 34 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// A direct database change outside the journal is not a write, so the projection keeps the
/// status of the last write. When the projection is also stale, every read is refused with an
/// explicit error, and backfill_board restores both the corrupted row and the projection.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_corrupted_test_list_and_a_stale_projection_recover_at_backfill() -> Result<()> {
    let w = world().await?;
    let task = "task:checked";
    w.command_as(&w.bot, json!({"op":"set_ac_tests","target":"ac:key-rights","tests":["checks.rs > key rights"],"reason":"Bind the checked test"})).await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"checked","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the specified work","owner":"owner","contract":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Check recovery under a green criterion"})).await?;
    w.command_as(&w.bot, json!({"op":"claim_task","id":task,"revision":1}))
        .await?;
    w.command_as(
        &w.bot,
        json!({"op":"complete_task","id":task,"revision":2,"evidence":"Implemented the contract"}),
    )
    .await?;
    let stamp = json!({"commit":"c4c4c4c","order":1,"branch":"master","default_branch":true,"dirty":false,"full":true});
    w.http
        .post(format!("{}/api/workspaces/{}/reports", w.root, w.ws))
        .bearer_auth(&w.bot)
        .json(&json!({"stamp":stamp,"junit":r#"<testsuites><testsuite name="checks.rs"><testcase name="key rights" /></testsuite></testsuites>"#}))
        .send()
        .await?
        .error_for_status()?;
    let passed = |object: &Value| {
        object["process"]["verification"] == "pass"
            && object["signal"]["tone"] == "green"
            && object["signal"]["label"] == "Work complete · Checks passed"
    };
    let before = w.get(&format!("address={task}")).await?;
    ensure!(passed(&before), "{before}");

    let row = "SELECT value::text FROM joppa.state_entries WHERE workspace=$1 AND section='ac_tests' AND key='ac:key-rights'";
    let original: String = w.pool.get().await?.query_one(row, &[&w.ws]).await?.get(0);
    let broken = w.pool.get().await?.execute(
        "UPDATE joppa.state_entries SET value='{\"revisions\":\"unreadable\"}'::jsonb WHERE workspace=$1 AND section='ac_tests' AND key='ac:key-rights'",
        &[&w.ws],
    ).await?;
    ensure!(broken == 1, "the saved list must exist");
    // The corruption is not a journal write: the read keeps the status of the last write.
    let kept = w.get(&format!("address={task}")).await?;
    ensure!(passed(&kept), "{kept}");

    // A stale projection is refused with an explicit error on HTTP and on MCP.
    w.pool.get().await?.execute(
        "UPDATE joppa.board_cache SET first_view=first_view-'status_projection' WHERE workspace=$1",
        &[&w.ws],
    ).await?;
    let reply = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/objects?address={task}",
            w.root, w.ws
        ))
        .bearer_auth(&w.owner)
        .send()
        .await?;
    ensure!(
        reply.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "HTTP {}",
        reply.status()
    );
    let body: Value = reply.json().await?;
    ensure!(
        body["error"] == "criterion status projection is not ready",
        "{body}"
    );
    let mcp: Value = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.owner)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":w.ws,"address":task}}}))
        .send().await?.error_for_status()?.json().await?;
    ensure!(
        mcp["result"]["isError"] == true
            && mcp["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|t| t.contains("criterion status projection is not ready")),
        "{mcp}"
    );

    // backfill_board restores the row from the journal and rebuilds the projection.
    store::backfill_board(&w.pool).await?;
    let restored: String = w.pool.get().await?.query_one(row, &[&w.ws]).await?.get(0);
    ensure!(
        restored == original,
        "restored {restored}, original {original}"
    );
    let after = w.get(&format!("address={task}")).await?;
    ensure!(passed(&after), "{after}");
    Ok(())
}
