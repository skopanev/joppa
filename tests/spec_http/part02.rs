// Part 2 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn completing_a_task_without_a_report_keeps_it_unverified() -> Result<()> {
    // done-is-not-green-ac1
    let w = world().await?;
    let mcp = |name: &str, arguments: Value| {
        let w = &w;
        let name = name.to_owned();
        async move {
            let response: Value = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.bot)
                .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}))
                .send().await?.error_for_status()?.json().await?;
            ensure!(response["result"]["isError"] != true, "{response}");
            Ok::<Value, anyhow::Error>(serde_json::from_str(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("MCP result needs text"))?,
            )?)
        }
    };
    let tree = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = tree["schema"]["types"].clone();
    types["job"] = types["task"].clone();
    types["job"]["title"] = json!("Job");
    w.command(json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"types":types,"reason":"Check another configured work type"})).await?;
    w.command_as(&w.bot, json!({"op":"set_ac_tests","target":"ac:key-rights","tests":["tests/spec_http.rs > completing_a_task_without_a_report_keeps_it_unverified"],"reason":"Wait for real test evidence"})).await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"requirement:key","revision":2,"ac":"ac:key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    for kind in ["task", "job"] {
        let address = format!("{kind}:unverified");
        w.command(json!({"op":"save_object","kind":kind,"id":"unverified","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the specified work","owner":"owner","body":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Check completion before any report"})).await?;
        w.command_as(&w.bot, json!({"op":"claim_task","id":address,"revision":1}))
            .await?;
        let claimed = mcp("joppa_objects", json!({"workspace":w.ws,"address":address})).await?;
        ensure!(
            claimed["process"]["status"] == "in_progress"
                && claimed["process"]["assignee"] == "bot",
            "{claimed}"
        );
        w.command_as(&w.bot, json!({"op":"complete_task","commit":"c0ffee0","id":address,"revision":claimed["revision"],"evidence":"Implemented the contract; test evidence is still absent"})).await?;
        let http: Value = w
            .http
            .get(format!("{}/api/workspaces/{}/objects", w.root, w.ws))
            .bearer_auth(&w.bot)
            .query(&[("address", &address)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let tool = mcp("joppa_objects", json!({"workspace":w.ws,"address":address})).await?;
        ensure!(
            crate::compact_of::compact_of(&http, &tool) == tool,
            "HTTP and MCP disagree: {http} / {tool}"
        );
        ensure!(
            http["process"]["status"] == "done"
                && http["process"].get("verification").is_none()
                && http["document"]["actor"] == "bot",
            "{http}"
        );
        let http_tests: Value = w
            .http
            .get(format!("{}/api/workspaces/{}/tests", w.root, w.ws))
            .bearer_auth(&w.bot)
            .query(&[("ac", "ac:key-rights")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let tool_tests = mcp(
            "joppa_tests",
            json!({"workspace":w.ws,"ac":"ac:key-rights"}),
        )
        .await?;
        ensure!(
            crate::compact_of::compact_of(&http_tests, &tool_tests) == tool_tests
                && http_tests["status"] == "grey",
            "HTTP and MCP criterion must stay grey: {http_tests} / {tool_tests}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_done_task_under_a_green_criterion_shows_checks_passed() -> Result<()> {
    // done-is-not-green-ac3
    let w = world().await?;
    let mcp = |name: &str, arguments: Value| {
        let w = &w;
        let name = name.to_owned();
        async move {
            let response: Value = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.alex)
                .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}))
                .send().await?.error_for_status()?.json().await?;
            ensure!(response["result"]["isError"] != true, "{response}");
            Ok::<Value, anyhow::Error>(serde_json::from_str(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("MCP result needs text"))?,
            )?)
        }
    };
    let task = "task:checked";
    w.command_as(&w.bot, json!({"op":"set_ac_tests","target":"ac:key-rights","tests":["checks.rs > key rights"],"reason":"Bind the checked test"})).await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"requirement:key","revision":2,"ac":"ac:key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"checked","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the specified work","owner":"owner","body":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Check completion under a green criterion"})).await?;
    w.command_as(&w.bot, json!({"op":"claim_task","id":task,"revision":1}))
        .await?;
    w.command_as(
        &w.bot,
        json!({"op":"complete_task","commit":"c0ffee0","id":task,"revision":2,"evidence":"Implemented the contract"}),
    )
    .await?;
    let passed = json!([{"test":"checks.rs > key rights","status":"pass"}]);
    record_http::recorded(&w.http, &w.root, &w.ws, &w.bot, "c3c3c3c", passed).await?;
    let object = mcp("joppa_objects", json!({"workspace":w.ws,"address":task})).await?;
    ensure!(
        object["process"]["status"] == "done"
            && object["process"].get("verification").is_none()
            && object["status"]["tone"] == "green"
            && object["status"]["label"] == "Work complete",
        "{object}"
    );
    let tests = mcp("joppa_tests", json!({"workspace":w.ws,"task":task})).await?;
    ensure!(
        tests["done"] == true && tests["commit"] == "c0ffee0",
        "{tests}"
    );
    let list = mcp(
        "joppa_objects",
        json!({"workspace":w.ws,"parent":"ac:key-rights","fields":["process.status","process.verification","signal.tone","signal.label"]}),
    )
    .await?;
    let row = list["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["address"] == task))
        .ok_or_else(|| anyhow::anyhow!("the list needs the task: {list}"))?;
    ensure!(
        row["process.status"] == "done"
            && row["process.verification"].is_null()
            && row["signal.tone"] == "green"
            && row["signal.label"] == "Work complete",
        "{row}"
    );
    // The Task queue reads the same state and signal.
    let queue = mcp("joppa_tasks", json!({"workspace":w.ws,"view":"all"})).await?;
    let entry = queue["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["address"] == task))
        .ok_or_else(|| anyhow::anyhow!("the queue needs the task: {queue}"))?;
    ensure!(
        entry["status"] == "done"
            && entry["verification"].is_null()
            && entry["signal"]["tone"] == "green"
            && entry["signal"]["label"] == "Work complete",
        "{entry}"
    );
    // A new test list that has not run makes the criterion grey, and T stays complete.
    w.command_as(&w.bot, json!({"op":"set_ac_tests","target":"ac:key-rights","tests":["checks.rs > key rights","checks.rs > new key check"],"reason":"Add a check that has not run"})).await?;
    let object = mcp("joppa_objects", json!({"workspace":w.ws,"address":task})).await?;
    let list = mcp(
        "joppa_objects",
        json!({"workspace":w.ws,"parent":"ac:key-rights","fields":["process.status","process.verification","signal.tone","signal.label"]}),
    )
    .await?;
    let row = list["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["address"] == task))
        .ok_or_else(|| anyhow::anyhow!("the list needs the task: {list}"))?;
    ensure!(
        object["process"]["status"] == "done"
            && object["process"].get("verification").is_none()
            && object["status"]["tone"] == "green"
            && object["status"]["label"] == "Work complete",
        "{object}"
    );
    ensure!(
        row["process.status"] == "done"
            && row["process.verification"].is_null()
            && row["signal.tone"] == "green"
            && row["signal.label"] == "Work complete",
        "{row}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_unreadable_task_criterion_keeps_a_mixed_list_readable() -> Result<()> {
    // done-is-not-green-ac3: the bound-run verdict stays when the criterion status cannot be read.
    let w = world().await?;
    w.requirement("aaa", "restrict", "Other rule", "Scope", &[("aaa-rule", "Keep the other rule")]).await?;
    w.command_as(&w.bot, json!({"op":"set_ac_tests","target":"ac:aaa-rule","tests":["checks.rs > other rule"],"reason":"Bind the checked test"})).await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"requirement:aaa","revision":2,"ac":"ac:aaa-rule","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"checked","revision":0,"parent":"ac:aaa-rule","data":{"title":"Implement the other rule","owner":"owner","body":"Implement the other rule and record the result","purpose":"implementation","projects":[]},"reason":"Check completion under an unreadable criterion"})).await?;
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"task:checked","revision":1}),
    )
    .await?;
    w.command_as(&w.bot, json!({"op":"complete_task","commit":"c0ffee0","id":"task:checked","revision":2,"evidence":"Implemented the contract"})).await?;
    let all = w.get("parent=*&limit=100").await?;
    let addresses: Vec<&str> = all["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["address"].as_str())
        .collect();
    let at = addresses
        .iter()
        .position(|a| *a == "requirement:key")
        .ok_or_else(|| anyhow::anyhow!("{all}"))?;
    ensure!(
        at > 0 && addresses[at - 1] == "requirement:aaa",
        "{addresses:?}"
    );
    let broken = w.pool.get().await?.execute(
        "UPDATE joppa.state_entries SET value='{\"revisions\":\"unreadable\"}'::jsonb WHERE workspace=$1 AND section='objects' AND key='ac:aaa-rule'",
        &[&w.ws],
    ).await?;
    ensure!(broken == 1, "the criterion document must exist");
    // The page holds requirement:key with its summary and the Task, but not requirement:aaa.
    let page = w
        .get(&format!(
            "parent=*&limit=2&cursor={}:requirement:aaa",
            all["position"]
        ))
        .await?;
    let (summary, task) = (&page["items"][0], &page["items"][1]);
    ensure!(
        summary["address"] == "requirement:key"
            && summary["verification_summary"]["total"] == 1
            && task["address"] == "task:checked"
            && task["process"]["status"] == "done"
            && task["process"].get("verification").is_none()
            && task["signal"]["tone"] == "green"
            && task["signal"]["label"] == "Work complete",
        "{page}"
    );
    Ok(())
}
