// Part 1 of tests/readonly_mcp_http.rs: MCP read tools and MCP write refusals for a viewer.

// ac:l98pa98e
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn viewer_reads_every_mcp_read_tool() -> Result<()> {
    let fake = fake_spaces::start();
    fake.configure();
    let w = record_world::world_env(&["tests/x.rs > t"], &fake.env()).await?;
    // Configure domain type on W, save domain:d and attach one file to it.
    let tree = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = tree["schema"]["types"].clone();
    types["domain"] = json!({"title":"Domains","role":"group","status":"none","root":true,"parents":[]});
    store::execute_remote_input(
        &w.pool,
        &w.ws,
        "owner",
        "schema-domain",
        &json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"types":types,"reason":"Add domain"}),
    )
    .await?;
    store::execute_remote_input(
        &w.pool,
        &w.ws,
        "bot",
        "save-d",
        &json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,"data":{"title":"Billing domain","owner":"owner","body":"Core billing scope"},"reason":"Fixture domain"}),
    )
    .await?;
    let file_body = "billing-attachment-bytes";
    let attached = w
        .mcp(
            &w.bot,
            "joppa_attach",
            json!({"target":"domain:d","filename":"spec.txt","content_type":"text/plain","content":file_body}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(attached["file"]["name"] == "spec.txt", "{attached}");
    let (_, maria_cookie, _) = enroll_viewer_maria(&w.pool, &w.ws).await?;
    let maria_mcp = oauth_token(&w, &maria_cookie).await?;

    // 1. initialize, 2. ping, 3. tools/list
    let init = mcp_rpc(
        &w,
        &maria_mcp,
        "initialize",
        json!({"protocolVersion":"2025-06-18"}),
    )
    .await?;
    ensure!(init["result"]["protocolVersion"].is_string(), "{init}");
    let ping = mcp_rpc(&w, &maria_mcp, "ping", json!({})).await?;
    ensure!(ping["result"].is_object(), "{ping}");
    let tools = mcp_rpc(&w, &maria_mcp, "tools/list", json!({})).await?;
    ensure!(
        tools["result"]["tools"]
            .as_array()
            .is_some_and(|t| t.len() == 12),
        "{tools}"
    );

    // 4. joppa_list, 5. joppa_tree, 6. joppa_objects, 7. joppa_search, 8. joppa_history,
    // 9. joppa_attachments, 10. joppa_help.
    let (code, listed_rpc) = w.mcp_raw(Some(&maria_mcp), "joppa_list", json!({})).await?;
    ensure!(
        code == 200 && listed_rpc["result"]["isError"] != true,
        "{listed_rpc}"
    );
    let listed: Value = serde_json::from_str(
        listed_rpc["result"]["content"][0]["text"]
            .as_str()
            .context("list text")?,
    )?;
    ensure!(
        listed["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|i| i["id"] == w.ws.as_str())),
        "{listed}"
    );
    let schema = w
        .mcp(&maria_mcp, "joppa_tree", json!({}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(schema["schema"]["types"]["domain"].is_object(), "{schema}");
    let card = w
        .mcp(&maria_mcp, "joppa_objects", json!({"address":"domain:d"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        card["data"]["title"] == "Billing domain" && card["files_count"] == 1,
        "{card}"
    );
    let found = w
        .mcp(&maria_mcp, "joppa_search", json!({"query":"Billing domain"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        found["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|i| i["address"] == "domain:d")),
        "{found}"
    );
    let history = w
        .mcp(&maria_mcp, "joppa_history", json!({"order":"newest"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        history["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "{history}"
    );
    let files = w
        .mcp(
            &maria_mcp,
            "joppa_attachments",
            json!({"target":"domain:d"}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
    let url = files["files"][0]["url"].as_str().context("attachment url")?;
    ensure!(reqwest::get(url).await?.text().await? == file_body);
    let (help_code, help_rpc) = w.mcp_raw(Some(&maria_mcp), "joppa_help", json!({})).await?;
    ensure!(
        help_code == 200
            && help_rpc["result"]["isError"] != true
            && help_rpc["result"]["content"][0]["text"].is_string(),
        "{help_rpc}"
    );
    Ok(())
}

// ac:pd01llbl
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn viewer_writes_nothing_over_mcp() -> Result<()> {
    let fake = fake_spaces::start();
    fake.configure();
    let w = record_world::world_env(&["tests/x.rs > t"], &fake.env()).await?;
    let tree = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = tree["schema"]["types"].clone();
    types["domain"] = json!({"title":"Domains","role":"group","status":"none","root":true,"parents":[]});
    store::execute_remote_input(
        &w.pool,
        &w.ws,
        "owner",
        "schema-domain",
        &json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"types":types,"reason":"Add domain"}),
    )
    .await?;
    store::execute_remote_input(
        &w.pool,
        &w.ws,
        "bot",
        "save-d",
        &json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,"data":{"title":"Domain D","owner":"owner","body":"Body"},"reason":"Fixture domain"}),
    )
    .await?;
    let (_, maria_cookie, _) = enroll_viewer_maria(&w.pool, &w.ws).await?;
    let maria_mcp = oauth_token(&w, &maria_cookie).await?;

    let before_pos: i64 = w
        .pool
        .get()
        .await?
        .query_one(
            "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    let open = w
        .mcp(&w.bot, "joppa_history", json!({"open":true}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let open_pos = open["items"][0]["position"].as_i64().context("open pos")?;

    let refusals = [
        w.mcp(
            &maria_mcp,
            "joppa_command",
            json!({"request_id":"v-new","command":{"op":"save_object","kind":"domain","id":"x","revision":0,"parent":null,"data":{"title":"X","owner":"owner"},"reason":"Try"}}),
        )
        .await?,
        w.mcp(
            &maria_mcp,
            "joppa_command",
            json!({"request_id":"v-same","command":{"op":"save_object","kind":"domain","id":"d","revision":1,"parent":null,"data":{"title":"Domain D","owner":"owner","body":"Body"},"reason":"Unchanged"}}),
        )
        .await?,
        w.mcp(
            &maria_mcp,
            "joppa_batch",
            json!({"request_id":"v-batch","operations":[{"command":{"op":"save_object","kind":"domain","id":"y","revision":0,"parent":null,"data":{"title":"Y","owner":"owner"},"reason":"Batch"}}]}),
        )
        .await?,
        w.mcp(
            &maria_mcp,
            "joppa_attach",
            json!({"target":"domain:d","filename":"no.txt","content":"nope"}),
        )
        .await?,
        w.mcp(
            &maria_mcp,
            "joppa_attach",
            json!({"target":"domain:d","filename":"no.txt","upload":true}),
        )
        .await?,
        w.mcp(
            &maria_mcp,
            "joppa_record_results",
            json!({"commit":"abc1234","results":[{"name":"tests/x.rs > t","outcome":"passed"}]}),
        )
        .await?,
        w.mcp(
            &maria_mcp,
            "joppa_history_mark",
            json!({"position":open_pos,"reason":"Try mark","nodes":["domain:d"]}),
        )
        .await?,
    ];
    for reply in refusals {
        let err = reply.expect_err("viewer write must be refused");
        ensure!(err == "a Read-only member cannot write", "got: {err}");
    }
    ensure!(
        !fake
            .keys()
            .iter()
            .any(|k| k.starts_with(&format!("workspaces/{}/", w.ws))),
        "Spaces got a file"
    );
    let conn = w.pool.get().await?;
    let row = conn
        .query_one(
            "SELECT COALESCE((SELECT max(position) FROM joppa.events WHERE workspace=$1),0),
                    (SELECT count(*) FROM joppa.test_reports WHERE workspace=$1),
                    (SELECT count(*) FROM joppa.history_marks WHERE workspace=$1)",
            &[&w.ws],
        )
        .await?;
    let (after_pos, reports, marks): (i64, i64, i64) = (row.get(0), row.get(1), row.get(2));
    ensure!(after_pos == before_pos && reports == 0 && marks == 0);
    Ok(())
}
