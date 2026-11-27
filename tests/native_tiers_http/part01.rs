// Part 1 of tests/native_tiers_http.rs: ac:zk5119jk (test:xrlx6klk and test:tz2ehs97).

// ac:zk5119jk
// test:xrlx6klk
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn data_sets_match_large_lists_scalars_nested_and_computed_fields() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_selector_schema(&w).await?;
    let cookie = session(&w, "alex").await?;
    let rkey = create_key(&w, &cookie, &w.ws).await?;

    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"domain","id":"pay","revision":0,"parent":null,"data":{"title":"Payments","owner":"owner","lane":"payments"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"domain","id":"idn","revision":0,"parent":null,"data":{"title":"Identity","owner":"owner","lane":"identity"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"requirement","id":"rp","revision":0,"parent":"domain:pay","data":{"title":"Pay Req","owner":"owner","body":"Rule"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"requirement","id":"ri","revision":0,"parent":"domain:idn","data":{"title":"Id Req","owner":"owner","body":"Rule"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"ac","id":"ap","revision":0,"parent":"requirement:rp","data":{"title":"Pay AC","owner":"owner"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"ac","id":"ai","revision":0,"parent":"requirement:ri","data":{"title":"Id AC","owner":"owner"},"reason":"Init"})).await?;

    // t_untagged has no tags at all and must survive data_none on tags.
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"t_untagged","revision":0,"parent":"ac:ap","data":{"title":"Alpha untagged selector target","owner":"owner","stage":"todo","delivery":{"state":"pending"}},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"t_both","revision":0,"parent":"ac:ap","data":{"title":"Alpha urgent retry selector target","owner":"owner","stage":"doing","tags":["urgent","kyc","extra"],"delivery":{"state":"retrying"}},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"t_skip","revision":0,"parent":"ac:ai","data":{"title":"Alpha historical closed target","owner":"owner","stage":"todo","tags":["done_450","kyc"],"delivery":{"state":"pending"}},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"t_done","revision":0,"parent":"ac:ai","data":{"title":"Alpha finished target","owner":"owner","stage":"done","tags":["urgent"],"delivery":{"state":"sent"}},"reason":"Init"})).await?;

    let skip_tags: Vec<String> = (0..600).map(|i| format!("done_{i}")).collect();
    let any_stage = json!({"stage": ["todo", "doing"], "delivery.state": ["pending", "retrying"]});
    let none_tags = json!({"tags": skip_tags});

    let mcp_res = mcp_ok(&w, &w.bot, "joppa_objects", json!({
        "kind": "task",
        "data_any": any_stage,
        "data_none": none_tags,
    })).await?;
    ensure!(
        item_addrs(&mcp_res) == ["task:t_both", "task:t_untagged"],
        "mcp data_any + 600-tag data_none (keeping untagged): {mcp_res}"
    );

    let q_str = format!(
        "kind=task&data_any={}&data_none={}",
        enc(&any_stage),
        enc(&none_tags)
    );
    let http_res = http_get_path(&w, "objects", &q_str).await?;
    ensure!(
        item_addrs(&http_res) == ["task:t_both", "task:t_untagged"],
        "http objects parity: {http_res}"
    );

    let cnt = mcp_ok(&w, &w.bot, "joppa_objects", json!({
        "kind": "task",
        "data_any": any_stage,
        "data_none": none_tags,
        "count": true,
    })).await?;
    ensure!(cnt["total"] == 2, "count parity: {cnt}");

    let grp = mcp_ok(&w, &w.bot, "joppa_objects", json!({
        "kind": "task",
        "data_any": any_stage,
        "data_none": none_tags,
        "group_by": ["stage"],
    })).await?;
    ensure!(grp["total"] == 2, "group_by parity: {grp}");

    // data_all on list tags + computed field lane on data_any.
    let all_res = mcp_ok(&w, &w.bot, "joppa_objects", json!({
        "kind": "task",
        "data_all": {"tags": ["kyc", "urgent"]},
        "data_any": {"lane": "payments"},
    })).await?;
    ensure!(item_addrs(&all_res) == ["task:t_both"], "data_all + computed lane: {all_res}");

    // POST /api/data parity.
    let api_res = data_read(&w, &rkey, json!({
        "kind": "task",
        "filter": {
            "data_any": any_stage,
            "data_none": none_tags,
        },
        "fields": ["title", "stage"]
    })).await?;
    let mut api_addrs = item_addrs(&api_res);
    api_addrs.sort();
    ensure!(
        api_addrs == ["task:t_both", "task:t_untagged"],
        "POST /api/data parity: {api_res}"
    );

    // Wait for search pending rows and verify joppa_search + HTTP /search parity.
    store::search_pending::pass(&w.pool).await?;
    let srch_mcp = mcp_ok(&w, &w.bot, "joppa_search", json!({
        "query": "Alpha",
        "kind": "task",
        "data_any": any_stage,
        "data_none": none_tags,
    })).await?;
    let mut srch_addrs = item_addrs(&srch_mcp);
    srch_addrs.sort();
    ensure!(
        srch_addrs == ["task:t_both", "task:t_untagged"],
        "joppa_search parity: {srch_mcp}"
    );
    let srch_http = http_get_path(
        &w,
        "search",
        &format!("q=Alpha&kind=task&data_any={}&data_none={}", enc(&any_stage), enc(&none_tags)),
    )
    .await?;
    let mut srch_http_addrs = item_addrs(&srch_http);
    srch_http_addrs.sort();
    ensure!(
        srch_http_addrs == ["task:t_both", "task:t_untagged"],
        "HTTP /search parity: {srch_http}"
    );
    Ok(())
}

// ac:zk5119jk
// test:tz2ehs97
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn data_sets_handle_empty_lists_and_refuse_invalid_shapes() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_selector_schema(&w).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"domain","id":"d1","revision":0,"parent":null,"data":{"title":"D1","owner":"owner","lane":"payments"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"domain","id":"d2","revision":0,"parent":null,"data":{"title":"D2","owner":"owner","lane":"identity"},"reason":"Init"})).await?;

    // Empty arrays: data_any -> 0, data_all -> all, data_none -> drops 0.
    let any_empty = mcp_ok(&w, &w.bot, "joppa_objects", json!({"kind":"domain","data_any":{"lane":[]}})).await?;
    ensure!(item_addrs(&any_empty).is_empty(), "empty data_any must keep 0: {any_empty}");
    let all_empty = mcp_ok(&w, &w.bot, "joppa_objects", json!({"kind":"domain","data_all":{"lane":[]}})).await?;
    ensure!(item_addrs(&all_empty) == ["domain:d1", "domain:d2"], "empty data_all must keep all: {all_empty}");
    let none_empty = mcp_ok(&w, &w.bot, "joppa_objects", json!({"kind":"domain","data_none":{"lane":[]}})).await?;
    ensure!(item_addrs(&none_empty) == ["domain:d1", "domain:d2"], "empty data_none must drop 0: {none_empty}");

    let too_many: Vec<String> = (0..10_001).map(|i| format!("v{i}")).collect();
    for bad in [
        json!({"data_any": ["not_an_object"]}),
        json!({"data_any": {"": ["a"]}}),
        json!({"data_any": {"a..b": ["a"]}}),
        json!({"data_all": {".a": ["a"]}}),
        json!({"data_none": {"tags": [123]}}),
        json!({"data_any": {"tags": too_many}}),
        json!({"address": "domain:d1", "data_any": {"lane": ["payments"]}}),
    ] {
        let err = w.mcp(&w.bot, "joppa_objects", bad.clone()).await?;
        ensure!(err.is_err(), "expected MCP refusal for {bad}, got {err:?}");
    }
    let (_, msg) = http_err(&w, &format!("address=domain:d1&data_any={}", enc(&json!({"lane":["payments"]})))).await?;
    ensure!(msg.contains("address cannot be combined with list filters"), "got {msg}");
    let (_, bad_json) = http_err(&w, "data_any=not_json").await?;
    ensure!(bad_json.contains("data_any filter must be an object"), "got {bad_json}");
    Ok(())
}
