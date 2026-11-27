// Part 1 of tests/p1_feedback_http.rs: ac:fgldeoxa and ac:9oh0z9aq.

// ac:fgldeoxa
// test:lrgvtdut
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p1_6_single_workspace_item_id_and_write_key_resolve_workspace() -> Result<()> {
    let fake = fake_spaces::start();
    let w_a = record_world::world_env(&[], &fake.env()).await?;
    let w_b = record_world::world_env(&[], &fake.env()).await?;
    let (solo_id, multi_id) = (
        format!("solo-{}", uuid::Uuid::new_v4().simple()),
        format!("multi-{}", uuid::Uuid::new_v4().simple()),
    );
    let member_op = |id: &str| serde_json::from_value::<Operation>(json!({"op":"member","id":id,"role":"human"}));
    store::execute(&w_a.pool, &w_a.ws, "owner", "enroll-solo", &member_op(&solo_id)?).await?;
    for ws in [&w_a.ws, &w_b.ws] {
        store::execute(&w_a.pool, ws, "owner", &format!("enroll-{ws}"), &member_op(&multi_id)?).await?;
    }
    let (solo_tok, multi_tok) = (oauth_token_for(&w_a, &solo_id).await?, oauth_token_for(&w_a, &multi_id).await?);

    // Solo caller reads and writes w_a without passing workspace.
    let tree = mcp_bare(&w_a, &solo_tok, "joppa_tree", json!({}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(tree["workspace_key"] == w_a.ws, "{tree}");
    let hist = mcp_bare(&w_a, &solo_tok, "joppa_history", json!({"limit":5}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(hist["items"].is_array(), "{hist}");
    mcp_bare(
        &w_a,
        &solo_tok,
        "joppa_command",
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"save_object","kind":"requirement","id":"solo_r","revision":0,"parent":null,"data":{"title":"Solo R","body":"Body","owner":"me"},"reason":"Solo write"}}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let objs = mcp_bare(
        &w_a,
        &solo_tok,
        "joppa_objects",
        json!({"address":"requirement:solo_r","fields":["owner"]}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    ensure!(objs["owner"] == solo_id, "{objs}");
    mcp_bare(
        &w_a,
        &solo_tok,
        "joppa_attach",
        json!({"target":"requirement:solo_r","filename":"a.txt","content":"hello"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let files = mcp_bare(
        &w_a,
        &solo_tok,
        "joppa_attachments",
        json!({"target":"requirement:solo_r"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    ensure!(files["files"].as_array().map(Vec::len) == Some(1), "{files}");

    // Multi caller resolves w_b via public item ID, and searches all_workspaces without workspace.
    let item_id_b: String = w_a
        .pool
        .get()
        .await?
        .query_one(
            "SELECT id FROM joppa.item_ids WHERE workspace=$1 AND kind='ac' AND local_id='a'",
            &[&w_b.ws],
        )
        .await?
        .get(0);
    let read_b = mcp_bare(
        &w_a,
        &multi_tok,
        "joppa_objects",
        json!({"address":item_id_b}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    ensure!(read_b["address"] == "ac:a", "{read_b}");
    let search_all = mcp_bare(
        &w_a,
        &multi_tok,
        "joppa_search",
        json!({"query":"Solo R","all_workspaces":true}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    ensure!(
        search_all["items"]
            .as_array()
            .is_some_and(|it| !it.is_empty()),
        "{search_all}"
    );

    // Write key resolves w_b without workspace.
    let jpt_b = create_write_key(&w_b).await?;
    let tree_b = mcp_bare(&w_b, &jpt_b, "joppa_tree", json!({}))
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(tree_b["workspace_key"] == w_b.ws, "{tree_b}");
    Ok(())
}

// ac:fgldeoxa
// test:qlr5cvhx
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p1_6_multi_and_zero_workspace_callers_name_choices_and_tools_list_omits_required()
-> Result<()> {
    let w_a = record_world::world_env(&[], &[]).await?;
    let w_b = record_world::world_env(&[], &[]).await?;
    let multi_id = format!("multi2-{}", uuid::Uuid::new_v4().simple());
    for ws in [&w_a.ws, &w_b.ws] {
        store::execute(
            &w_a.pool,
            ws,
            "owner",
            &format!("enroll-{ws}"),
            &serde_json::from_value::<Operation>(
                json!({"op":"member","id":multi_id,"role":"human"}),
            )?,
        )
        .await?;
    }
    let multi_tok = oauth_token_for(&w_a, &multi_id).await?;
    let mut sorted = [w_a.ws.clone(), w_b.ws.clone()];
    sorted.sort();
    let expected = format!("missing workspace: choose one of {}", sorted.join(", "));
    let err = mcp_bare(&w_a, &multi_tok, "joppa_tree", json!({}))
        .await?
        .expect_err("multi must refuse");
    ensure!(err == expected && !err.contains(';'), "got {err:?}");

    let zero_err = mcp_bare(&w_a, &w_a.outsider, "joppa_tree", json!({}))
        .await?
        .expect_err("zero must refuse");
    ensure!(zero_err == "missing workspace", "got {zero_err:?}");

    let raw = w_a
        .http
        .post(format!("{}/mcp", w_a.root))
        .bearer_auth(&w_a.bot)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await?
        .text()
        .await?;
    ensure!(raw.len() <= 13_950, "tools/list bytes: {}", raw.len());
    let parsed: Value = serde_json::from_str(&raw)?;
    for tool in parsed["result"]["tools"].as_array().into_iter().flatten() {
        let req = tool["inputSchema"]["required"].as_array();
        ensure!(
            req.is_none_or(|r| !r.contains(&json!("workspace"))),
            "tool {} still requires workspace: {tool}",
            tool["name"]
        );
    }
    Ok(())
}

// ac:9oh0z9aq
// test:idgu1p19
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p1_7_search_title_boost_ranks_exact_substring_and_punctuation_hits_first() -> Result<()> {
    let w = record_world::world(&[]).await?;
    ok(
        &w,
        json!({"op":"save_object","kind":"ac","id":"u","revision":0,"parent":"requirement:r",
            "data":{"title":"The tools/list reply is at most 13500 bytes","body":"Given: MCP server\nWhen: tools/list runs\nThen: payload stays lean","owner":"owner"},"reason":"Create U"}),
    )
    .await?;
    for i in 1..=12 {
        ok(
            &w,
            json!({"op":"save_object","kind":"ac","id":format!("comp{i:02}"),"revision":0,"parent":"requirement:r",
                "data":{"title":format!("Criterion {i}"),"body":"Given: reply bytes\nWhen: most payload runs\nThen: reply is at most 13500 bytes in budget","owner":"owner"},"reason":"Create competitor"}),
        )
        .await?;
    }

    let punct = mcp_ok(
        &w,
        &w.bot,
        "joppa_search",
        json!({"query":"tools/list budget","kind":"ac"}),
    )
    .await?;
    ensure!(
        punct["items"]
            .as_array()
            .is_some_and(|it| it.iter().any(|h| h["address"] == "ac:u")),
        "tools/list budget missed u: {punct}"
    );

    let exact_page1 = mcp_ok(
        &w,
        &w.bot,
        "joppa_search",
        json!({"query":"THE TOOLS/LIST REPLY IS AT MOST 13500 BYTES","kind":"ac","limit":1}),
    )
    .await?;
    ensure!(
        exact_page1["items"][0]["address"] == "ac:u",
        "exact title page 1: {exact_page1}"
    );
    let (code, http_search) = call(
        &w.http,
        &w.root,
        "GET",
        &format!("/api/workspaces/{}/search?q=THE%20TOOLS%2FLIST%20REPLY%20IS%20AT%20MOST%2013500%20BYTES&kind=ac&limit=1", w.ws),
        &[Auth::Bearer(&w.bot)],
        None,
    )
    .await?;
    ensure!(
        code == 200
            && http_search["items"][0]["id"] == "u"
            && http_search["items"][0]["score"].as_f64().unwrap_or(0.0) >= 0.10,
        "http search score: {code} {http_search}"
    );

    let sub_page1 = mcp_ok(
        &w,
        &w.bot,
        "joppa_search",
        json!({"query":"tools/list reply is at most","kind":"ac","limit":1}),
    )
    .await?;
    ensure!(
        sub_page1["items"][0]["address"] == "ac:u",
        "substring page 1: {sub_page1}"
    );

    let exact_addr = mcp_ok(&w, &w.bot, "joppa_search", json!({"query":"ac:u"})).await?;
    ensure!(
        exact_addr["items"].as_array().map(Vec::len) == Some(1)
            && exact_addr["items"][0]["address"] == "ac:u",
        "{exact_addr}"
    );
    Ok(())
}
