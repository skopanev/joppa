// Part 1 of tests/p0_feedback_http.rs: ac:9vb5eaq3, ac:3qhus6yb and ac:izklhyhv.

// ac:9vb5eaq3
// test:aj3hfsjm
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p0_1_command_schema_and_save_title_body_guard() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let raw = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await?
        .text()
        .await?;
    ensure!(
        raw.len() <= 13_950,
        "tools/list exceeded 13950 bytes: {}",
        raw.len()
    );
    let parsed: Value = serde_json::from_str(&raw)?;
    let tools = parsed["result"]["tools"]
        .as_array()
        .context("tools array")?;
    let cmd_tool = tools
        .iter()
        .find(|t| t["name"] == "joppa_command")
        .context("joppa_command")?;
    let props = &cmd_tool["inputSchema"]["properties"]["command"]["properties"];
    ensure!(
        props.get("title").is_none() && props.get("body").is_none(),
        "title or body leaked in joppa_command properties: {props}"
    );
    let data_desc = props["data"]["description"].as_str().unwrap_or_default();
    ensure!(
        data_desc.contains("title")
            && data_desc.contains("body")
            && data_desc.contains("owner")
            && !data_desc.contains(';'),
        "unexpected data description: {data_desc}"
    );

    let pos = head(&w).await?;
    for bad in [
        json!({"op":"save_object","kind":"requirement","id":"r1","revision":0,"parent":null,"title":"Outside","data":{"title":"Inside","owner":"owner"},"reason":"Bad title"}),
        json!({"op":"save_object","kind":"requirement","id":"r1","revision":0,"parent":null,"body":"Outside","data":{"title":"Inside","owner":"owner"},"reason":"Bad body"}),
    ] {
        let mcp_res = w
            .mcp(
                &w.bot,
                "joppa_command",
                json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":bad}),
            )
            .await?;
        refused(mcp_res, "put title and body inside data")?;
            let http_res = save(&w, bad).await?;
        refused(http_res, "put title and body inside data")?;
    }
    ensure!(
        head(&w).await? == pos,
        "journal advanced on refused save_object"
    );

    ok(
        &w,
        json!({"op":"save_object","kind":"requirement","id":"r1","revision":0,"parent":null,
            "data":{"title":"Inside","body":"Clean body","owner":"owner"},"reason":"Valid save"}),
    )
    .await?;
    read_key::command(
        &w,
        &w.owner,
        json!({"op":"update_workspace","title":"Renamed Workspace"}),
    )
    .await?;
    Ok(())
}

// ac:3qhus6yb
// test:ad5zvy67
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p0_2_stale_revision_errors_over_mcp_and_batch() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_p0_schema(&w).await?;
    ok(
        &w,
        json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,
            "data":{"title":"D","owner":"owner"},"reason":"Create D"}),
    )
    .await?;
    ok(
        &w,
        json!({"op":"save_object","kind":"capability","id":"c","revision":0,"parent":"domain:d",
            "data":{"title":"C","owner":"owner"},"reason":"Create C"}),
    )
    .await?;

    let pos = head(&w).await?;
    let check_err = |res: std::result::Result<Value, String>, expected: &str| -> Result<()> {
        let err = res.expect_err("expected stale revision error");
        ensure!(
            err.contains(expected) && !err.contains(';'),
            "got {err:?}, expected {expected:?}"
        );
        Ok(())
    };

    let s1 = save(
        &w,
        json!({"op":"save_object","kind":"capability","id":"c","revision":3,"data":{"title":"C2"},"reason":"Stale"}),
    )
    .await?;
    check_err(s1, "stale object revision on capability:c: sent 3, current is 1")?;

    let s2 = save(
        &w,
        json!({"op":"save_object","kind":"capability","id":"n","revision":2,"parent":"domain:d","data":{"title":"N","owner":"owner"},"reason":"New with rev 2"}),
    )
    .await?;
    check_err(s2, "stale object revision on capability:n: sent 2, current is 0")?;

    let a_bad = w
        .command(
            &w.owner,
            json!({"op":"archive_object","target":"capability:c","revision":3,"reason":"Stale archive"}),
        )
        .await?;
    check_err(a_bad, "stale object revision on capability:c: sent 3, current is 1")?;
    ensure!(head(&w).await? == pos, "journal advanced on stale writes");

    w.command(
        &w.owner,
        json!({"op":"archive_object","target":"capability:c","revision":1,"reason":"Archive C"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let pos_archived = head(&w).await?;

    let r_bad = w
        .command(
            &w.owner,
            json!({"op":"restore_object","target":"capability:c","revision":1,"reason":"Stale restore"}),
        )
        .await?;
    check_err(r_bad, "stale object revision on capability:c: sent 1, current is 2")?;

    let batch_res = w
        .mcp(
            &w.bot,
            "joppa_batch",
            json!({
                "request_id": uuid::Uuid::new_v4().to_string(),
                "operations": [
                    {"command":{"op":"save_object","kind":"capability","id":"ok1","revision":0,"parent":"domain:d","data":{"title":"OK","owner":"owner"},"reason":"Op 1"}},
                    {"command":{"op":"save_object","kind":"capability","id":"c","revision":1,"data":{"title":"Stale"},"reason":"Op 2"}}
                ]
            }),
        )
        .await?;
    check_err(
        batch_res,
        "operation 2: stale object revision on capability:c: sent 1, current is 2",
    )?;
    ensure!(head(&w).await? == pos_archived, "failed batch advanced journal");
    Ok(())
}

// ac:izklhyhv
// test:aucz3n1j
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p0_3_comment_page_limit_and_boundaries() -> Result<()> {
    let w = record_world::world(&[]).await?;
    ok(
        &w,
        json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":"requirement:r",
            "data":{"title":"AC b","body":"Given: x\nWhen: y\nThen: z","owner":"owner"},"reason":"Create AC b"}),
    )
    .await?;
    let mut ops = Vec::new();
    for i in 1..=30 {
        ops.push(json!({"command":{"op":"note","target":"ac:a","id":format!("c{i:02}"),"text":format!("Note {i}")}}));
    }
    for i in 1..=3 {
        ops.push(json!({"command":{"op":"note","target":"ac:b","id":format!("b{i:02}"),"text":format!("B {i}")}}));
    }
    ops.push(json!({"command":{"op":"withdraw_note","target":"ac:a","id":"c02","reason":"Drop 2"}}));
    ops.push(json!({"command":{"op":"withdraw_note","target":"ac:a","id":"c05","reason":"Drop 5"}}));
    mcp_ok(
        &w,
        &w.bot,
        "joppa_batch",
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"operations":ops}),
    )
    .await?;

    let p1 = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"ac:a","fields":["comments"],"limit":10})).await?;
    ensure!(p1["comments"]["items"].as_array().map(Vec::len) == Some(10) && p1["comments"]["total"] == 30 && p1["comments"]["next_cursor"] == "10", "{p1}");
    let p2 = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"ac:a","fields":["comments"],"limit":10,"comments_cursor":"10"})).await?;
    ensure!(p2["comments"]["items"].as_array().map(Vec::len) == Some(10) && p2["comments"]["next_cursor"] == "20", "{p2}");
    let p3 = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"ac:a","fields":["comments"],"limit":10,"comments_cursor":"20"})).await?;
    ensure!(p3["comments"]["items"].as_array().map(Vec::len) == Some(10) && p3["comments"].get("next_cursor").is_none(), "{p3}");

    let active = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"ac:a","fields":["comments_active"],"limit":10})).await?;
    ensure!(active["comments_active"]["items"].as_array().map(Vec::len) == Some(10) && active["comments_active"]["total"] == 28, "{active}");

    for args in [
        json!({"address":"ac:a","fields":["comments"],"limit":40}),
        json!({"address":"ac:a","fields":["comments"]}),
    ] {
        let clamped = mcp_ok(&w, &w.bot, "joppa_objects", args).await?;
        ensure!(clamped["comments"]["items"].as_array().map(Vec::len) == Some(25), "{clamped}");
    }
    let zero = w.mcp(&w.bot, "joppa_objects", json!({"address":"ac:a","fields":["comments"],"limit":0})).await?;
    refused(zero, "limit must be at least 1")?;

    let many = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"ac:a,ac:b","fields":["comments"],"limit":10})).await?;
    ensure!(many["items"][0]["comments"]["items"].as_array().map(Vec::len) == Some(25), "{many}");
    Ok(())
}
