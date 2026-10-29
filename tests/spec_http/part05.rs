// Part 5 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// One completion by `token` over HTTP or MCP at the current revision; checks the exact refusal
/// and that the Task and every stored row, including the journal position, are unchanged.
/// Returns the Task as read before the attempt.
async fn refused_completion(w: &World, token: &str, address: &str, mcp: bool) -> Result<Value> {
    let task = w.get(&format!("address={address}")).await?;
    let saved = saved_rows(w).await?;
    let complete = json!({"op":"complete_task","commit":"c0ffee0","id":address,"revision":task["current_revision"],"evidence":"Implemented the contract and recorded checks"});
    let request = json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":complete});
    let error = if mcp {
        let result = mcp_call(
            w,
            token,
            "joppa_command",
            json!({"workspace":w.ws,"request_id":request["request_id"],"command":complete}),
        )
        .await?;
        ensure!(result["isError"] == true, "{result}");
        result["content"][0]["text"].clone()
    } else {
        let response = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(token)
            .json(&request)
            .send()
            .await?;
        ensure!(
            response.status() == reqwest::StatusCode::CONFLICT,
            "{}",
            response.status()
        );
        response.json::<Value>().await?["error"].clone()
    };
    ensure!(error == "Task is not claimed unfinished work", "{error}");
    let unchanged = w.get(&format!("address={address}")).await?;
    ensure!(
        unchanged["current_revision"] == task["current_revision"]
            && unchanged["process"]["verification"] == task["process"]["verification"]
            && unchanged["process"] == task["process"]
            && unchanged["document"] == task["document"],
        "The refused completion changed the Task: {unchanged}"
    );
    let rows = saved_rows(w).await?;
    ensure!(
        rows["position"] == saved["position"] && rows == saved,
        "The refused completion changed stored rows"
    );
    Ok(task)
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn blank_completion_evidence_is_refused_over_http_and_mcp() -> Result<()> {
    // completion-record-ac3
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    w.command_as(bot, json!({"op":"assess","req":"requirement:key","revision":2,"ac":"ac:key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"blank-evidence","revision":0,"parent":"ac:key-rights","data":{"title":"Complete the defined work","owner":"owner","contract":"Implement the key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Check blank completion evidence"})).await?;
    w.command_as(
        bot,
        json!({"op":"claim_task","id":"task:blank-evidence","revision":1}),
    )
    .await?;
    let claimed = w.get("address=task:blank-evidence").await?;
    ensure!(
        claimed["process"]["status"] == "in_progress"
            && claimed["process"]["assignee"] == "bot"
            && claimed["process"]["done"].is_null(),
        "{claimed}"
    );
    let conn = w.pool.get().await?;
    let before: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    let request_id = uuid::Uuid::new_v4().to_string();
    let complete = json!({"op":"complete_task","commit":"c0ffee0","id":"task:blank-evidence","revision":claimed["current_revision"],"evidence":"  "});
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(bot)
        .json(&json!({"request_id":request_id,"command":complete}))
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::CONFLICT,
        "{}",
        response.status()
    );
    let response: Value = response.json().await?;
    ensure!(response["error"] == "text must not be empty", "{response}");
    let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":request_id,"command":complete}}})).send().await?;
    ensure!(response.status().is_success(), "{}", response.status());
    let response: Value = response.json().await?;
    ensure!(
        response["result"]["isError"] == true
            && response["result"]["content"][0]["text"] == "text must not be empty",
        "{response}"
    );
    let unchanged = w.get("address=task:blank-evidence").await?;
    ensure!(
        unchanged["process"]["status"] == "in_progress"
            && unchanged["process"]["assignee"] == "bot"
            && unchanged["process"]["done"].is_null()
            && unchanged["current_revision"] == claimed["current_revision"]
            && unchanged["process"] == claimed["process"],
        "The refused completion changed the Task: {unchanged}"
    );
    let after: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(before == after, "The refused completion appended an event");
    let receipts: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2",
            &[&w.ws, &request_id],
        )
        .await?
        .get(0);
    ensure!(
        receipts == 0,
        "The refused completion consumed its request ID"
    );
    Ok(())
}
