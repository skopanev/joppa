// Part 24 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// Every joppa_objects page with parent=*, following next_cursor to the end.
async fn all_object_pages(w: &World, token: &str) -> Result<Vec<Value>> {
    let mut items = vec![];
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let mut arguments = json!({"workspace":w.ws,"parent":"*","limit":2});
        if let Some(cursor) = &cursor {
            arguments["cursor"] = json!(cursor);
        }
        let result = mcp_call(w, token, "joppa_objects", arguments).await?;
        ensure!(result["isError"] != true, "{result}");
        let page: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap_or(""))?;
        items.extend(page["items"].as_array().cloned().unwrap_or_default());
        pages += 1;
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    ensure!(pages > 1, "the fixture must fill more than one page");
    Ok(items)
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_read_only_agent_token_reads_the_workspace() -> Result<()> {
    // read-only-agent-token-ac1
    let w = world().await?;
    for id in ["first", "second", "third"] {
        w.note(id, 0, id, "Input", json!([])).await?;
    }
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/agent-tokens", w.root, w.ws))
        .bearer_auth(&w.owner)
        .json(&json!({"agent":"planner-reader","access":"read"}))
        .send()
        .await?;
    ensure!(reply.status().is_success(), "{}", reply.text().await?);
    let created: Value = reply.json().await?;
    ensure!(created["access"] == "read", "{created}");
    let reader = created["token"].as_str().unwrap().to_owned();
    ensure!(reader.starts_with("jpt_"), "{created}");
    let role: Option<String> = w
        .pool
        .get()
        .await?
        .query_opt(
            "SELECT value#>>'{}' FROM joppa.state_entries WHERE workspace=$1 AND section='members' AND key='planner-reader'",
            &[&w.ws],
        )
        .await?
        .map(|row| row.get(0));
    ensure!(role.as_deref() == Some("agent"), "{role:?}");
    let grant = store::token_actor(&w.pool, &reader).await?.unwrap();
    ensure!(
        grant.agent == "planner-reader"
            && grant.workspace == w.ws
            && grant.access == store::Access::Read,
        "{grant:?}"
    );

    // Every allowlisted tool reads and stores nothing.
    let before = stored_counts(&w).await?;
    for (name, arguments) in [
        ("joppa_list", json!({})),
        (
            "joppa_search",
            json!({"workspace":w.ws,"query":"Key rights"}),
        ),
        ("joppa_read", json!({"workspace":w.ws})),
        (
            "joppa_tests",
            json!({"workspace":w.ws,"ac":"ac:key-rights"}),
        ),
        ("joppa_tree", json!({"workspace":w.ws})),
        ("joppa_objects", json!({"workspace":w.ws,"parent":"*"})),
        ("joppa_tasks", json!({"workspace":w.ws})),
        ("joppa_help", json!({})),
        ("joppa_help", json!({"workspace":w.ws})),
        ("joppa_model_docs", json!({})),
    ] {
        let result = mcp_call(&w, &reader, name, arguments).await?;
        ensure!(result["isError"] != true, "{name}: {result}");
    }
    let owner_items = all_object_pages(&w, &w.owner).await?;
    let reader_items = all_object_pages(&w, &reader).await?;
    ensure!(
        owner_items == reader_items,
        "{owner_items:?} / {reader_items:?}"
    );
    ensure!(stored_counts(&w).await? == before, "a read stored data");

    // Write tools and unknown tools are refused before any store call.
    for (name, arguments) in [
        (
            "joppa_command",
            json!({"workspace":w.ws,"request_id":"read-only-command","command":{"op":"save_object","kind":"note","id":"refused","revision":0,"parent":null,"data":{"title":"Refused","owner":"owner","body":"Input"},"reason":"Save"}}),
        ),
        (
            "joppa_batch",
            json!({"workspace":w.ws,"request_id":"read-only-batch","operations":[{"command":{"op":"save_object","kind":"note","id":"refused","revision":0,"parent":null,"data":{"title":"Refused","owner":"owner","body":"Input"},"reason":"Save"}}]}),
        ),
        ("joppa_unknown", json!({"workspace":w.ws})),
    ] {
        let result = mcp_call(&w, &reader, name, arguments).await?;
        ensure!(
            result["isError"] == true
                && result["content"][0]["text"]
                    .as_str()
                    .is_some_and(|t| t == "read-only token"),
            "{name}: {result}"
        );
    }
    ensure!(
        stored_counts(&w).await? == before,
        "a refused tool stored data"
    );

    // Every HTTP route that changes state refuses the read grant.
    let id = created["id"].as_i64().unwrap();
    let api = format!("{}/api/workspaces/{}", w.root, w.ws);
    for (path, body) in [
        ("/commands".to_owned(), json!({"request_id":"read-only-http","command":{"op":"save_object","kind":"note","id":"refused","revision":0,"parent":null,"data":{"title":"Refused","owner":"owner","body":"Input"},"reason":"Save"}}).to_string()),
        ("/order".to_owned(), json!({"request_id":"read-only-order","kind":"note","parent":null,"expected":[],"ids":[]}).to_string()),
        ("/agent-tokens".to_owned(), json!({"agent":"another-reader"}).to_string()),
        (format!("/agent-tokens/{id}/revoke"), String::new()),
        ("/blobs".to_owned(), "file body".to_owned()),
    ] {
        let reply = w
            .http
            .post(format!("{api}{path}"))
            .bearer_auth(&reader)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await?;
        let status = reply.status().as_u16();
        let text = reply.text().await?;
        ensure!(
            status == 403
                && serde_json::from_str::<Value>(&text)
                    .is_ok_and(|v| v["error"] == "read-only token"),
            "{path}: {status} {text}"
        );
    }
    ensure!(
        stored_counts(&w).await? == before,
        "a refused route stored data"
    );
    let read = w
        .http
        .get(format!("{api}/objects?parent=*"))
        .bearer_auth(&reader)
        .send()
        .await?;
    ensure!(read.status().is_success(), "{}", read.status());

    // A revoked read token is refused.
    let reply = w
        .http
        .post(format!("{api}/agent-tokens/{id}/revoke"))
        .bearer_auth(&w.owner)
        .send()
        .await?;
    let status = reply.status().as_u16();
    let text = reply.text().await?;
    ensure!(status == 200, "revoke: {status} {text}");
    let reply = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&reader)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_tree","arguments":{"workspace":w.ws}}}))
        .send()
        .await?;
    let status = reply.status().as_u16();
    let text = reply.text().await?;
    ensure!(
        status == 401
            && serde_json::from_str::<Value>(&text).is_ok_and(|v| v["error"] == "token revoked"),
        "{status} {text}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn tools_list_for_a_read_only_token_lists_only_the_read_tools() -> Result<()> {
    // read-only-agent-token-ac5
    let w = world().await?;
    let mut names = vec![];
    for (agent, access) in [("planner-reader", "read"), ("control-agent", "write")] {
        let created: Value = w
            .http
            .post(format!("{}/api/workspaces/{}/agent-tokens", w.root, w.ws))
            .bearer_auth(&w.owner)
            .json(&json!({"agent":agent,"access":access}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let token = created["token"].as_str().context("token")?;
        let listed: Value = w
            .http
            .post(format!("{}/mcp", w.root))
            .bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let tools: Vec<String> = listed["result"]["tools"]
            .as_array()
            .context("tools")?
            .iter()
            .filter_map(|t| t["name"].as_str().map(String::from))
            .collect();
        names.push(tools);
    }
    let (read, write) = (&names[0], &names[1]);
    let writes = [
        "joppa_command",
        "joppa_batch",
        "joppa_record_results",
        "joppa_history_mark",
    ];
    ensure!(
        writes.iter().all(|n| write.iter().any(|t| t == n)),
        "{write:?}"
    );
    ensure!(
        !read.is_empty() && read.iter().all(|t| !writes.contains(&t.as_str())),
        "{read:?}"
    );
    ensure!(
        read.iter().all(|t| write.contains(t)) && read.len() + writes.len() == write.len(),
        "{read:?} {write:?}"
    );
    Ok(())
}
