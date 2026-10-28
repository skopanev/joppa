// Part 23 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn legacy_revision_cannot_bypass_a_configured_type_question() -> Result<()> {
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["requirement"]["question"] = json!("Which behavior does the product need?");
    types["domain"]["question"] = json!("Which product area does this object describe?");
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check the requirement question"})).await?;
    let response = w.http.post(format!("{}/api/workspaces/{}/commands", w.root, w.ws)).bearer_auth(&w.owner)
        .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"domain","id":"new","title":"New product area","authority":"owner"}})).send().await?;
    ensure!(response.status().as_u16() == 422, "{}", response.status());
    ensure!(
        response.json::<Value>().await?["error"]
            == "This type has a question. Use save_object with question_checked",
        "wrong creation question error"
    );
    let exists: bool = w.pool.get().await?.query_one("SELECT EXISTS(SELECT 1 FROM joppa.objects WHERE workspace=$1 AND address='domain:new')", &[&w.ws]).await?.get(0);
    ensure!(!exists, "a refused legacy creation left an object");
    let original = w.get("address=requirement:key").await?;
    let command = json!({"op":"revise","req":"key","expected_revision":1,"capability":"restrict","title":"Updated rights","body":"Updated scope","owner":"owner","consumers":["customer"],"reason":"Revise the legacy contract","acs":[{"id":"key-rights","text":"Only read and verify before launch"}]});
    let request_id = uuid::Uuid::new_v4().to_string();
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id":request_id,"command":command}))
        .send()
        .await?;
    ensure!(response.status().as_u16() == 422, "{}", response.status());
    ensure!(
        response.json::<Value>().await?["error"]
            == "This type has a question. Use save_object with question_checked",
        "wrong legacy question error"
    );
    ensure!(
        w.get("address=requirement:key").await?["document"] == original["document"],
        "a refused legacy revision changed the object"
    );
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["requirement"]
        .as_object_mut()
        .unwrap()
        .remove("question");
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Keep the old path for types without a question"})).await?;
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id":request_id,"command":command}))
        .send()
        .await?;
    ensure!(response.status().is_success(), "{}", response.text().await?);
    let updated = w.get("address=requirement:key").await?;
    ensure!(
        updated["document"]["data"]["title"] == "Updated rights"
            && updated["current_revision"] == original["current_revision"].as_u64().unwrap() + 1,
        "{updated}"
    );
    Ok(())
}

// mod claim_time is declared in tests/spec_http.rs, so its #[path] resolves from tests/.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn permanent_claims_and_heartbeat_refusals_agree_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    claim_time::install(&w.pool).await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["job"] = json!({"title":"Jobs","role":"task","status":"none","root":true,"parents":["*"]});
    w.command(json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Use configured work"})).await?;
    w.command(json!({"op":"save_object","kind":"job","id":"one","revision":0,"parent":null,"data":{"title":"Implement","owner":"owner","contract":"Implement and check the contract","purpose":"implementation"},"reason":"Create work"})).await?;
    let response=w.http.post(format!("{}/api/workspaces/{}/commands",w.root,w.ws)).bearer_auth(&w.bot).json(&json!({"request_id":format!("test-claim-time-86400000-{}",uuid::Uuid::new_v4().simple()),"command":{"op":"claim_task","id":"job:one","revision":1}})).send().await?;
    ensure!(response.status().is_success(), "{}", response.text().await?);
    let read_mcp = async |name: &str, arguments: Value| -> Result<Value> {
        let reply:Value=w.http.post(format!("{}/mcp",w.root)).bearer_auth(&w.bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":(crate::legacy_read_args(name, &arguments))}})).send().await?.json().await?;
        ensure!(reply["result"]["isError"] != true, "{reply}");
        Ok(serde_json::from_str(
            reply["result"]["content"][0]["text"].as_str().unwrap(),
        )?)
    };
    let before = w.get("address=job:one").await?;
    for view in ["all", "mine", "available"] {
        let http: Value = w
            .http
            .get(format!(
                "{}/api/workspaces/{}/tasks?view={view}",
                w.root, w.ws
            ))
            .bearer_auth(&w.bot)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mcp = read_mcp("joppa_tasks", json!({"workspace":w.ws,"view":view})).await?;
        ensure!(http["tasks"] == mcp["tasks"], "HTTP {http} / MCP {mcp}");
        if view == "available" {
            ensure!(http["total"] == 0, "held work is unavailable");
        } else {
            ensure!(
                http["tasks"][0]["status"] == "in_progress"
                    && http["tasks"][0]["assignee"] == "bot",
                "{http}"
            );
        }
    }
    let command = json!({"op":"heartbeat_task","id":"job:one","revision":2});
    let http: Value = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.bot)
        .json(&json!({"request_id":"refused-http","command":command}))
        .send()
        .await?
        .json()
        .await?;
    ensure!(
        http["error"] == "heartbeat_task is no longer supported",
        "{http}"
    );
    let mcp:Value=w.http.post(format!("{}/mcp",w.root)).bearer_auth(&w.bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":"refused-mcp","command":command}}})).send().await?.json().await?;
    ensure!(
        mcp["result"]["isError"] == true
            && mcp["result"]["content"][0]["text"] == "heartbeat_task is no longer supported",
        "{mcp}"
    );
    ensure!(
        before == w.get("address=job:one").await?,
        "refusals change no state"
    );
    let help = read_mcp("joppa_help", json!({"workspace":w.ws})).await?;
    ensure!(
        !help["operations"]
            .as_object()
            .unwrap()
            .contains_key("heartbeat_task"),
        "help does not advertise heartbeat writes"
    );
    store::check_journal(&w.pool).await?;
    Ok(())
}

/// One MCP tools/call; returns the JSON-RPC result.
async fn mcp_call(w: &World, token: &str, name: &str, arguments: Value) -> Result<Value> {
    let reply: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":(crate::legacy_read_args(name, &arguments))}}))
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
                    (SELECT count(*) FROM joppa.blobs),
                    (SELECT count(*) FROM joppa.test_reports),
                    (SELECT count(*) FROM joppa.command_receipts)",
            &[&w.ws],
        )
        .await?;
    Ok((0..6).map(|i| row.get(i)).collect())
}

/// Committed rows that a write can change, each in a stable order: the workspace journal and
/// its position, current state with members, receipts, tokens, reports and all file blobs.
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
        "task_queue",
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
                    (SELECT COALESCE(jsonb_agg(jsonb_build_array(hash,size,created_at) ORDER BY hash),'[]')::text FROM joppa.blobs),
                    (SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1)",
            &[&w.ws],
        )
        .await?;
    for (i, name) in ["agent_token_revocations", "test_results", "blobs"]
        .into_iter()
        .enumerate()
    {
        saved[name] = serde_json::from_str(row.get(i))?;
    }
    saved["position"] = json!(row.get::<_, i64>(3));
    Ok(saved)
}
