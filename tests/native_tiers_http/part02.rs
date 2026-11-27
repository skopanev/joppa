// Part 2 of tests/native_tiers_http.rs: ac:ow9ghazl (test:ugeqo4um and test:3h7bvstd).

// ac:ow9ghazl
// test:ugeqo4um
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn tiers_rank_urgent_first_then_domain_queue_with_cursor_and_counts() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_selector_schema(&w).await?;

    for (id, lane) in [("billing", "payments"), ("auth", "identity")] {
        cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"domain","id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"owner","lane":lane},"reason":"Init"})).await?;
        cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"requirement","id":format!("r_{id}"),"revision":0,"parent":format!("domain:{id}"),"data":{"title":id,"owner":"owner","body":"Rule"},"reason":"Init"})).await?;
        cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"ac","id":format!("a_{id}"),"revision":0,"parent":format!("requirement:r_{id}"),"data":{"title":id,"owner":"owner"},"reason":"Init"})).await?;
    }

    // b_low (dependents 0, older), b_high (dependents 1, newer), b_wait (blocked on b_high),
    // b_closed (excluded by data_none), auth_urg (urgent in other domain -> tier 0!), auth_norm (non-urgent in auth).
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"b_low","revision":0,"parent":"ac:a_billing","data":{"title":"Billing low","owner":"owner","stage":"todo"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"b_high","revision":0,"parent":"ac:a_billing","data":{"title":"Billing high","owner":"owner","stage":"todo"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"b_wait","revision":0,"parent":"ac:a_billing","data":{"title":"Billing blocked","owner":"owner","stage":"todo"},"relations":{"depends_on":{"edges":[{"to":"task:b_high"}]}},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"b_closed","revision":0,"parent":"ac:a_billing","data":{"title":"Billing closed","owner":"owner","stage":"todo","tags":["skip_me"]},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"auth_urg","revision":0,"parent":"ac:a_auth","data":{"title":"Auth urgent","owner":"owner","stage":"todo","tags":["urgent"]},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"auth_norm","revision":0,"parent":"ac:a_auth","data":{"title":"Auth normal","owner":"owner","stage":"doing"},"reason":"Init"})).await?;

    let tiers = json!([
        {"data_any": {"tags": ["urgent"]}},
        {"parent": "domain:billing", "depth": 5}
    ]);
    let full = mcp_ok(&w, &w.bot, "joppa_objects", json!({
        "kind": "task",
        "blocked": false,
        "data_none": {"tags": ["skip_me"]},
        "tiers": tiers,
        "order": "dependents desc, age asc",
        "fields": ["title", "dependents"]
    })).await?;
    ensure!(
        item_addrs(&full) == ["task:auth_urg", "task:b_high", "task:b_low"],
        "tier 0 urgent must beat tier 1 higher dependents, and auth_norm must be dropped: {full}"
    );

    // HTTP parity + snapshot cursor pagination across tier boundary.
    let q_base = format!(
        "kind=task&blocked=false&data_none={}&tiers={}&order=dependents%20desc%2C%20age%20asc",
        enc(&json!({"tags":["skip_me"]})),
        enc(&tiers),
    );
    let http_full = http_get_path(&w, "objects", &q_base).await?;
    ensure!(item_addrs(&http_full) == ["task:auth_urg", "task:b_high", "task:b_low"], "http tiers: {http_full}");

    let p1 = mcp_ok(&w, &w.bot, "joppa_objects", json!({
        "kind": "task",
        "blocked": false,
        "data_none": {"tags": ["skip_me"]},
        "tiers": tiers,
        "order": "dependents desc, age asc",
        "limit": 2
    })).await?;
    ensure!(item_addrs(&p1) == ["task:auth_urg", "task:b_high"], "p1: {p1}");
    let cur = p1["next_cursor"].as_str().context("next_cursor")?;
    ensure!(cur.contains(":@2"), "expected snapshot offset cursor, got {cur}");

    let p2 = mcp_ok(&w, &w.bot, "joppa_objects", json!({
        "kind": "task",
        "blocked": false,
        "data_none": {"tags": ["skip_me"]},
        "tiers": tiers,
        "order": "dependents desc, age asc",
        "limit": 2,
        "cursor": cur
    })).await?;
    ensure!(item_addrs(&p2) == ["task:b_low"] && p2.get("next_cursor").is_none(), "p2: {p2}");

    // Catch-all {} tier includes auth_norm in tier 1, and count/group_by match.
    let catch_all = json!([{"data_any": {"tags": ["urgent"]}}, {}]);
    let all_open = mcp_ok(&w, &w.bot, "joppa_objects", json!({
        "kind": "task",
        "blocked": false,
        "data_none": {"tags": ["skip_me"]},
        "tiers": catch_all
    })).await?;
    ensure!(
        item_addrs(&all_open) == ["task:auth_urg", "task:auth_norm", "task:b_high", "task:b_low"],
        "catch-all tier order: {all_open}"
    );
    let cnt = mcp_ok(&w, &w.bot, "joppa_objects", json!({
        "kind": "task",
        "blocked": false,
        "data_none": {"tags": ["skip_me"]},
        "tiers": tiers,
        "count": true
    })).await?;
    ensure!(cnt["total"] == 3, "tiers count: {cnt}");
    let grp = mcp_ok(&w, &w.bot, "joppa_objects", json!({
        "kind": "task",
        "blocked": false,
        "data_none": {"tags": ["skip_me"]},
        "tiers": catch_all,
        "group_by": ["stage"]
    })).await?;
    ensure!(grp["total"] == 4, "tiers group_by: {grp}");

    // Write invalidates snapshot cursor.
    cmd_ok(&w, &w.bot, json!({"op":"note","target":"task:b_low","id":"n1","text":"Advance snapshot"})).await?;
    let stale = w.mcp(&w.bot, "joppa_objects", json!({
        "kind": "task",
        "tiers": tiers,
        "cursor": cur
    })).await?;
    ensure!(stale.is_err(), "stale snapshot cursor must fail, got {stale:?}");
    Ok(())
}

// ac:ow9ghazl
// test:3h7bvstd
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn tiers_refuse_nested_invalid_bounds_and_non_list_modes() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_selector_schema(&w).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"domain","id":"d1","revision":0,"parent":null,"data":{"title":"D1","owner":"owner","lane":"payments"},"reason":"Init"})).await?;

    let eleven: Vec<Value> = (0..11).map(|_| json!({"kind": "domain"})).collect();
    for (bad, needle) in [
        (json!({"tiers": "bad"}), "tiers takes 1 to 10 filter objects"),
        (json!({"tiers": []}), "tiers takes 1 to 10 filter objects"),
        (json!({"tiers": eleven}), "tiers takes 1 to 10 filter objects"),
        (json!({"tiers": ["bad"]}), "each tier must be a filter object"),
        (json!({"tiers": [{"tiers": [{}]}]}), "tiers cannot nest tiers"),
        (json!({"address": "domain:d1", "tiers": [{}]}), "tiers applies only to object lists and counts"),
        (json!({"relations_of": "domain:d1", "tiers": [{}]}), "tiers applies only to object lists and counts"),
        (json!({"in_relation": "depends_on", "tiers": [{}]}), "tiers applies only to object lists and counts"),
    ] {
        let err = w.mcp(&w.bot, "joppa_objects", bad.clone()).await?;
        let text = err.expect_err("expected refusal").to_string();
        ensure!(text.contains(needle), "expected `{needle}` for {bad}, got `{text}`");
    }
    let (_, http_msg) = http_err(&w, &format!("address=domain:d1&tiers={}", enc(&json!([{}])))).await?;
    ensure!(http_msg.contains("tiers applies only to object lists and counts"), "got {http_msg}");
    Ok(())
}
