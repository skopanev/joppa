// Part 2 of tests/own_requirement_revisions_http.rs: ac:rghuniww (nominal and off-nominal).

// ac:rghuniww (nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn history_under_returns_subtree_entries_by_address_alias_and_item_id() -> Result<()> {
    let w = record_world::world(&[]).await?;
    mcp_ok(
        &w,
        "joppa_history_mark",
        json!({"position": 1, "reason": "Processed initial workspace event", "nodes": []}),
    )
    .await?;

    // Sibling subtree requirement:other > ac:other_ac must be excluded from under requirement:sub.
    ok(
        &w,
        json!({"op": "save_object", "kind": "requirement", "id": "other", "revision": 0, "parent": null, "data": {"title": "Other", "owner": "owner", "body": "Other"}, "reason": "Seed other"}),
    )
    .await?;
    ok(
        &w,
        json!({"op": "save_object", "kind": "ac", "id": "other_ac", "revision": 0, "parent": "requirement:other", "data": {"title": "Other AC", "owner": "owner"}, "reason": "Seed other ac"}),
    )
    .await?;

    // Target subtree requirement:sub > (ac:sub_a > test:sub_t, archived ac:sub_b) plus comment on ac:sub_a.
    ok(
        &w,
        json!({"op": "save_object", "kind": "requirement", "id": "sub", "revision": 0, "parent": null, "data": {"title": "Subtree R", "owner": "owner", "body": "Root"}, "reason": "Create sub"}),
    )
    .await?;
    ok(
        &w,
        json!({"op": "save_object", "kind": "ac", "id": "sub_a", "revision": 0, "parent": "requirement:sub", "data": {"title": "Sub A", "owner": "owner"}, "reason": "Create sub_a"}),
    )
    .await?;
    ok(
        &w,
        json!({"op": "save_object", "kind": "test", "id": "sub_t", "revision": 0, "parent": "ac:sub_a", "data": {"title": "Sub T", "owner": "owner"}, "reason": "Create sub_t"}),
    )
    .await?;
    ok(
        &w,
        json!({"op": "save_object", "kind": "ac", "id": "sub_b", "revision": 0, "parent": "requirement:sub", "data": {"title": "Sub B", "owner": "owner"}, "reason": "Create sub_b"}),
    )
    .await?;
    ok(
        &w,
        json!({"op": "archive_object", "target": "ac:sub_b", "revision": 1, "reason": "Archive sub_b"}),
    )
    .await?;
    ok(
        &w,
        json!({"op": "note", "target": "ac:sub_a", "id": "c1", "text": "Subtree comment"}),
    )
    .await?;

    let sub_first = mcp_ok(&w, "joppa_history", json!({"target": "requirement:sub"})).await?;
    let sub_pos = sub_first["items"][0]["position"].as_i64().unwrap();
    mcp_ok(
        &w,
        "joppa_history_mark",
        json!({"position": sub_pos, "reason": "Processed sub root", "nodes": ["requirement:sub"]}),
    )
    .await?;

    w.pool
        .get()
        .await?
        .execute(
            "INSERT INTO joppa.object_aliases(workspace, address, target) VALUES ($1, 'req:old', 'requirement:sub'), ($1, 'ac:old', 'ac:sub_a')",
            &[&w.ws],
        )
        .await?;
    let sub_public_id = item_id(&w, "requirement", "sub").await?;

    let positions = |reply: &Value| -> Vec<i64> {
        reply["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["position"].as_i64().unwrap())
            .collect()
    };
    let addresses = |reply: &Value| -> Vec<String> {
        reply["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["address"].as_str().unwrap().to_owned())
            .collect()
    };

    let by_addr = mcp_ok(&w, "joppa_history", json!({"under": "requirement:sub"})).await?;
    let by_req_alias = mcp_ok(&w, "joppa_history", json!({"under": "req:old"})).await?;
    let by_item_id = mcp_ok(&w, "joppa_history", json!({"under": &sub_public_id})).await?;
    ensure!(
        positions(&by_addr) == positions(&by_req_alias)
            && positions(&by_addr) == positions(&by_item_id),
        "address, alias and item ID differed: {by_addr} vs {by_req_alias} vs {by_item_id}"
    );
    ensure!(
        addresses(&by_addr)
            == vec![
                "requirement:sub",
                "ac:sub_a",
                "test:sub_t",
                "ac:sub_b",
                "ac:sub_b",
                "ac:sub_a",
            ],
        "unexpected subtree addresses: {:?}",
        addresses(&by_addr)
    );
    ensure!(by_addr["total"] == 6 && by_addr["next_item"] == 2, "{by_addr}");

    let under_a = mcp_ok(&w, "joppa_history", json!({"under": "ac:sub_a"})).await?;
    let under_a_alias = mcp_ok(&w, "joppa_history", json!({"under": "ac:old"})).await?;
    ensure!(
        positions(&under_a) == positions(&under_a_alias)
            && addresses(&under_a) == vec!["ac:sub_a", "test:sub_t", "ac:sub_a"]
            && under_a["total"] == 3,
        "under A: {under_a}"
    );

    let mut paged = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut args = json!({"under": "requirement:sub", "limit": 2});
        if let Some(c) = &cursor {
            args["cursor"] = json!(c);
        }
        let page = mcp_ok(&w, "joppa_history", args).await?;
        ensure!(page["total"] == 6, "page total: {page}");
        paged.extend(positions(&page));
        cursor = page["next_cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    ensure!(paged == positions(&by_addr), "paged: {paged:?}");

    let http_reply: Value = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/history?under=requirement:sub",
            w.root, w.ws
        ))
        .bearer_auth(&w.bot)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(
        positions(&http_reply) == positions(&by_addr)
            && http_reply["total"] == 6
            && http_reply["items"][0].get("signal").is_some(),
        "http reply: {http_reply}"
    );

    let narrowed = mcp_ok(
        &w,
        "joppa_history",
        json!({"under": "requirement:sub", "open": true, "op": "save_object"}),
    )
    .await?;
    ensure!(
        addresses(&narrowed) == vec!["ac:sub_a", "test:sub_t", "ac:sub_b"]
            && narrowed["total"] == 3
            && narrowed["next_item"] == 2,
        "narrowed: {narrowed}"
    );
    Ok(())
}

// ac:rghuniww (off-nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn history_under_rejects_empty_or_non_string_and_isolates_unknown_ids() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let other = record_world::world(&[]).await?;
    let foreign_item_id = item_id(&other, "requirement", "r").await?;

    for bad in [json!({"under": ""}), json!({"under": 42})] {
        let res = w.mcp(&w.bot, "joppa_history", bad).await?;
        refused(res, "a History filter is a non-empty text")?;
    }

    let http_empty = w
        .http
        .get(format!("{}/api/workspaces/{}/history?under=", w.root, w.ws))
        .bearer_auth(&w.bot)
        .send()
        .await?;
    ensure!(http_empty.status() == 422, "expected 422: {http_empty:?}");

    let isolated = mcp_ok(&w, "joppa_history", json!({"under": foreign_item_id})).await?;
    ensure!(
        isolated["total"] == 0 && isolated["items"] == json!([]),
        "foreign item ID leaked: {isolated}"
    );
    Ok(())
}
