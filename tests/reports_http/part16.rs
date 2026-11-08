// Part 16 of tests/reports_http.rs: the MCP helpers of the HTTP checks.

/// Call one MCP tool and return the raw JSON-RPC reply. Transport and decoding errors fail here.
async fn mcp(w: &World, token: &str, tool: &str, arguments: Value) -> Result<Value> {
    let root = w.base.split("/api/").next().unwrap();
    Ok(w
        .http
        .post(format!("{root}/mcp"))
        .bearer_auth(token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}}))
        .send()
        .await?
        .json()
        .await?)
}

/// A JSON-RPC result without isError=true gives its decoded text content.
fn read_result(tool: &str, reply: &Value) -> Result<Value> {
    let result = &reply["result"];
    ensure!(result.is_object(), "{tool}: no result: {reply}");
    let text = result["content"][0]["text"].as_str();
    ensure!(
        result["isError"] != true,
        "{tool}: {}",
        text.unwrap_or_default()
    );
    let text = text.ok_or_else(|| anyhow::anyhow!("{tool}: no text: {reply}"))?;
    Ok(serde_json::from_str(text)?)
}

async fn mcp_read(w: &World, token: &str, tool: &str, arguments: Value) -> Result<Value> {
    read_result(tool, &mcp(w, token, tool, arguments).await?)
}

