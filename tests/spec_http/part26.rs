// Part 26 of tests/spec_http.rs, included there, so the test names stay unchanged.

// [ac:criterion-checkable-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_bot_criterion_title_over_64_characters_is_refused() -> Result<()> {
    let w = world().await?;
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/agent-tokens", w.root, w.ws))
        .bearer_auth(&w.owner)
        .json(&json!({"agent":"bot","access":"write"}))
        .send()
        .await?;
    ensure!(reply.status().is_success(), "{}", reply.text().await?);
    let created: Value = reply.json().await?;
    ensure!(created["access"] == "write", "{created}");
    let token = created["token"].as_str().unwrap().to_owned();
    let grant = store::token_actor(&w.pool, &token).await?.unwrap();
    ensure!(
        grant.agent == "bot" && grant.workspace == w.ws && grant.access == store::Access::Write,
        "{grant:?}"
    );
    let read = |address: &'static str| {
        let (w, token) = (&w, &token);
        async move {
            let reply = w
                .http
                .get(format!("{}/api/workspaces/{}/objects", w.root, w.ws))
                .bearer_auth(token)
                .query(&[("address", address)])
                .send()
                .await?;
            let status = reply.status().as_u16();
            Ok::<_, anyhow::Error>((status, reply.json::<Value>().await?))
        }
    };
    let criterion = |id: &str, title: String| json!({"op":"save_object","kind":"ac","id":id,"revision":0,"parent":"requirement:key","data":{"title":title,"owner":"owner","consumer":"customer","description":"Given: a requirement.\nWhen: bot saves a criterion.\nThen: the title fits.\nEvidence: the saved criterion."},"reason":"Check the title length"});

    let (status, saved) = text_rule_request(
        &w,
        &token,
        "fits-title",
        criterion("fits", "a".repeat(64)),
        "http",
    )
    .await?;
    ensure!(status == 200, "{status}: {saved}");
    let (status, fits) = read("ac:fits").await?;
    ensure!(
        status == 200 && fits["document"]["data"]["title"] == "a".repeat(64),
        "{status}: {fits}"
    );

    let conn = w.pool.get().await?;
    let counts = || async {
        let row = conn
            .query_one(
                "SELECT (SELECT count(*) FROM joppa.events WHERE workspace=$1),
                        (SELECT count(*) FROM joppa.object_revisions WHERE workspace=$1),
                        (SELECT count(*) FROM joppa.command_receipts WHERE workspace=$1)",
                &[&w.ws],
            )
            .await?;
        Ok::<_, anyhow::Error>((0..3).map(|i| row.get(i)).collect::<Vec<i64>>())
    };
    let before = counts().await?;
    let (status, denied) = text_rule_request(
        &w,
        &token,
        "long-title",
        criterion("long", "a".repeat(65)),
        "http",
    )
    .await?;
    ensure!(
        !(200..300).contains(&status)
            && denied["error"]
                == "an ac title is at most 64 characters; put the details in the text",
        "{status}: {denied}"
    );
    ensure!(
        counts().await? == before,
        "a refused title stored an event, revision or receipt"
    );
    let (status, missing) = read("ac:long").await?;
    ensure!(
        !(200..300).contains(&status) && missing == json!({"error":"unknown object"}),
        "the refused criterion is readable: {status}: {missing}"
    );
    Ok(())
}

/// One REST command; returns whether it succeeded and the reply body.
async fn send_as(w: &World, token: &str, command: &Value) -> Result<(bool, Value)> {
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(token)
        .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
        .send()
        .await?;
    Ok((reply.status().is_success(), reply.json().await?))
}

/// The JSON value of one successful MCP tools/call.
async fn mcp_value(w: &World, name: &str, arguments: Value) -> Result<Value> {
    let result = mcp_call(w, &w.bot, name, arguments).await?;
    ensure!(result["isError"] != true, "{result}");
    Ok(serde_json::from_str(
        result["content"][0]["text"].as_str().unwrap_or("null"),
    )?)
}

/// Bot search over MCP; fails on HTTP, JSON-RPC or tool errors. Returns addresses and the reply.
async fn bot_search(w: &World, query: &str) -> Result<(Vec<String>, Value)> {
    let result = mcp_call(
        w,
        &w.bot,
        "joppa_search",
        json!({"workspace":w.ws,"query":query}),
    )
    .await?;
    search_addresses(&result)
}

/// Addresses of a joppa_search tool result; a tool error or a reply without a result array fails.
fn search_addresses(result: &Value) -> Result<(Vec<String>, Value)> {
    ensure!(result["isError"] != true, "tool error: {result}");
    let text = result["content"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no text: {result}"))?;
    let found: Value = serde_json::from_str(text).map_err(|e| anyhow::anyhow!("{e}: {result}"))?;
    let rows = found["items"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("no result array: {result}"))?;
    let addresses = rows
        .iter()
        .map(|r| match (r["kind"].as_str(), r["id"].as_str()) {
            (Some(kind), Some(id)) => Ok(format!("{kind}:{id}")),
            _ => anyhow::bail!("result without an address: {result}"),
        })
        .collect::<Result<_>>()?;
    Ok((addresses, found))
}

#[test]
fn search_addresses_rejects_a_tool_error_with_the_reply() {
    let reply = json!({"isError":true,"content":[{"type":"text","text":"search failed"}]});
    let err = search_addresses(&reply).unwrap_err().to_string();
    assert!(err.contains("search failed"), "{err}");
}

#[test]
fn search_addresses_rejects_a_reply_without_a_result_array() {
    let reply = json!({"content":[{"type":"text","text":"{\"mode\":\"full_text\"}"}]});
    let err = search_addresses(&reply).unwrap_err().to_string();
    assert!(
        err.contains("no result array") && err.contains("full_text"),
        "{err}"
    );
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn help_operations_equal_the_command_op_enum() -> Result<()> {
    use std::collections::BTreeSet;
    let w = world().await?;
    let reply: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let tools = reply["result"]["tools"].as_array().unwrap();
    let schema = |name: &str| {
        tools
            .iter()
            .find(|t| t["name"] == name)
            .map(|t| t["inputSchema"].clone())
            .unwrap_or_default()
    };
    let names = |command: &Value| -> BTreeSet<String> {
        command["properties"]["op"]["enum"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    };
    let command = names(&schema("joppa_command")["properties"]["command"]);
    let batch =
        names(&schema("joppa_batch")["properties"]["operations"]["items"]["properties"]["command"]);
    ensure!(
        !command.is_empty() && batch == command,
        "{command:?} / {batch:?}"
    );
    for name in [
        "heartbeat_task",
        "workspace",
        "enroll_member",
        "enroll_service",
        "migrate_persistent_claims",
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
        "migrate_task_claims",
        "record_report",
    ] {
        ensure!(!command.contains(name), "{name} is in the op enum");
    }
    for arguments in [json!({}), json!({"workspace":w.ws})] {
        let help = mcp_value(&w, "joppa_help", arguments.clone()).await?;
        let operations = help["operations"].as_object().unwrap();
        let keys: BTreeSet<String> = operations.keys().cloned().collect();
        ensure!(keys == command, "help {keys:?} / enum {command:?}");
        for (name, entry) in operations {
            let callers = entry["callers"].as_array().unwrap();
            ensure!(
                !callers.is_empty()
                    && callers.iter().all(|c| {
                        ["member", "agent", "holder", "workspace owner"]
                            .contains(&c.as_str().unwrap_or(""))
                    })
                    && entry["required"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(Value::is_string),
                "{name}: {entry}"
            );
        }
        ensure!(
            help["search"].is_string()
                && help["writing"].is_string()
                && help["objects"].is_string()
                && help.get("tree").is_some() == arguments.get("workspace").is_some(),
            "other help fields stay"
        );
    }
    Ok(())
}
