// Part 34 of tests/reports_http.rs, included there, so the test names stay unchanged.

// task-no-purpose
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn claim_next_task_and_joppa_tasks_have_no_purpose() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.ready().await?;
    p.save("work:t0", Some("criterion:a"), json!({"title":"T0","owner":"owner","body":"Implement this criterion"}), json!([])).await?;
    p.refuse_command(
        "claim_next_task with a purpose",
        json!({"op":"claim_next_task","purpose":"implementation"}),
        "unknown claim_next_task field",
    )
    .await?;
    let root = w.base.split("/api/").next().unwrap();
    let list: Value = w.http.post(format!("{root}/mcp")).bearer_auth(&w.token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send().await?.json().await?;
    let tasks = list["result"]["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|t| t["name"] == "joppa_tasks"))
        .cloned()
        .unwrap_or_default();
    ensure!(
        tasks["inputSchema"]["properties"].is_object()
            && tasks["inputSchema"]["properties"].get("purpose").is_none(),
        "{tasks}"
    );
    let mcp = p.mcp("joppa_tasks", json!({"view":"all"})).await?;
    let page: Value = serde_json::from_str(
        mcp["result"]["content"][0]["text"].as_str().unwrap_or_default(),
    )?;
    let items = task_list(&page).as_array().cloned().unwrap_or_default();
    ensure!(
        !items.is_empty() && items.iter().all(|t| t.get("purpose").is_none()),
        "{page}"
    );
    let reply = p
        .send(true, &json!({"op":"claim_next_task"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let t0 = p.read("work:t0").await?;
    ensure!(
        t0["process"]["assignee"] == "agent"
            && t0["process"].get("purpose").is_none()
            && t0["document"]["data"]["title"] == "T0"
            && t0.pointer("/document/data/purpose").is_none(),
        "{reply} {t0}"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}
