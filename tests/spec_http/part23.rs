// Part 23 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// One MCP tools/call; returns the JSON-RPC result.
async fn mcp_call(w: &World, token: &str, name: &str, arguments: Value) -> Result<Value> {
    let reply: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(reply.get("error").is_none(), "{reply}");
    Ok(reply["result"].clone())
}

/// Counts of everything a write can change: journal events, tokens, files and reports.
async fn stored_counts(w: &World) -> Result<Vec<i64>> {
    let conn = w.pool.get().await?;
    let row = conn
        .query_one(
            "SELECT (SELECT count(*) FROM joppa.events WHERE workspace=$1),
                    (SELECT count(*) FROM joppa.agent_tokens),
                    (SELECT count(*) FROM joppa.agent_token_revocations),
                    (SELECT count(*) FROM joppa.test_reports),
                    (SELECT count(*) FROM joppa.command_receipts)",
            &[&w.ws],
        )
        .await?;
    let mut counts: Vec<i64> = (0..5).map(|i| row.get(i)).collect();
    counts.push(fake_spaces::start().keys().len() as i64);
    Ok(counts)
}

/// Committed rows that a write can change, each in a stable order: the workspace journal and
/// its position, current state with members, receipts, tokens, reports and all stored files.
async fn saved_rows(w: &World) -> Result<Value> {
    let conn = w.pool.get().await?;
    let mut saved = json!({});
    // Table names are fixed literals; SQL cannot bind identifiers.
    for table in [
        "events",
        "objects",
        "object_revisions",
        "object_links",
        "state_entries",
        "state_revisions",
        "board_cache",
        "command_receipts",
        "agent_tokens",
        "test_reports",
    ] {
        let rows: String = conn
            .query_one(
                &format!("SELECT COALESCE(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)),'[]')::text FROM joppa.{table} t WHERE workspace=$1"),
                &[&w.ws],
            )
            .await?
            .get(0);
        saved[table] = serde_json::from_str(&rows)?;
    }
    let row = conn
        .query_one(
            "SELECT (SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)),'[]')::text
                       FROM joppa.agent_token_revocations r JOIN joppa.agent_tokens t ON t.id=r.token_id WHERE t.workspace=$1),
                    (SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)),'[]')::text
                       FROM joppa.test_results r JOIN joppa.test_reports t ON t.id=r.report_id WHERE t.workspace=$1),
                    (SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1)",
            &[&w.ws],
        )
        .await?;
    for (i, name) in ["agent_token_revocations", "test_results"]
        .into_iter()
        .enumerate()
    {
        saved[name] = serde_json::from_str(row.get(i))?;
    }
    saved["position"] = json!(row.get::<_, i64>(2));
    saved["files"] = json!(fake_spaces::start().keys());
    Ok(saved)
}
