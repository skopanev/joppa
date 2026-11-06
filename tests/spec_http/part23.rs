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

/// Committed rows that a write can change, each in a stable order: the workspace journal and
/// its position, current state with members, receipts, reports and all stored files.
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
                       FROM joppa.test_results r JOIN joppa.test_reports t ON t.id=r.report_id WHERE t.workspace=$1),
                    (SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1)",
            &[&w.ws],
        )
        .await?;
    saved["test_results"] = serde_json::from_str(row.get(0))?;
    saved["position"] = json!(row.get::<_, i64>(1));
    saved["files"] = json!(fake_spaces::start().keys());
    Ok(saved)
}
