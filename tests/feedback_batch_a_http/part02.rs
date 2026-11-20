// Part 2 of tests/feedback_batch_a_http.rs: ac:a3qp8jxw, ac:mhuj6ldr, ac:7b8q9bq5.

// ac:a3qp8jxw
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn address_read_handles_100_items_with_missing_and_names_single_unknown() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let tree = ok(&w, "joppa_tree", json!({})).await?;
    w.command(
        &w.owner,
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Add domain type",
            "put_types": {
                "domain": {"title": "Domain", "role": "group", "status": "children", "root": true, "parents": []}
            }
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    let ops: Vec<Value> = (0..98)
        .map(|i| {
            json!({"command": {"op": "save_object", "kind": "domain", "id": format!("d{i:02}"), "revision": 0, "parent": null, "data": {"title": format!("D{i:02}"), "owner": "owner"}, "reason": "Seed"}})
        })
        .collect();
    ok(
        &w,
        "joppa_batch",
        json!({"request_id": "seed-98-domains", "operations": ops}),
    )
    .await?;

    let mut asked: Vec<String> = (0..98).map(|i| format!("domain:d{i:02}")).collect();
    let existing = ok(
        &w,
        "joppa_objects",
        json!({"address": asked.join(","), "fields": ["title"]}),
    )
    .await?;
    ensure!(
        existing["items"].as_array().map(Vec::len) == Some(98)
            && existing.get("missing").is_none(),
        "{existing}"
    );

    asked.push("domain:x".into());
    asked.push("joppa-D-NOPE1234".into());
    let mixed = ok(
        &w,
        "joppa_objects",
        json!({"address": asked.join(","), "fields": ["title"]}),
    )
    .await?;
    let items = mixed["items"].as_array().context("mixed items")?;
    ensure!(
        items.len() == 98
            && items[0]["address"] == "domain:d00"
            && items[97]["address"] == "domain:d97"
            && mixed["missing"] == json!(["domain:x", "joppa-D-NOPE1234"]),
        "{mixed}"
    );

    asked.push("domain:d00".into());
    let over = w
        .mcp(&w.bot, "joppa_objects", json!({"address": asked.join(",")}))
        .await?;
    ensure!(
        over == Err("at most 100 addresses in one call".into()),
        "{over:?}"
    );

    let single = w
        .mcp(&w.bot, "joppa_objects", json!({"address": "domain:x"}))
        .await?;
    ensure!(
        single == Err("unknown object: domain:x".into()),
        "{single:?}"
    );
    let single_id = w
        .mcp(
            &w.bot,
            "joppa_objects",
            json!({"address": "joppa-D-NOPE1234"}),
        )
        .await?;
    ensure!(
        single_id == Err("unknown object: joppa-D-NOPE1234".into()),
        "{single_id:?}"
    );
    Ok(())
}

// ac:mhuj6ldr
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn history_pages_default_to_25_and_cap_at_100() -> Result<()> {
    let w = record_world::world(&[]).await?;
    for (part, chunk) in [(0, 0..75), (1, 75..150)] {
        let ops: Vec<Value> = chunk
            .map(|i| {
                json!({"command": {"op": "note", "target": "ac:a", "id": format!("h{i:03}"), "text": format!("Entry {i}")}})
            })
            .collect();
        ok(
            &w,
            "joppa_batch",
            json!({"request_id": format!("hist-seed-{part}"), "operations": ops}),
        )
        .await?;
    }

    let p25 = ok(&w, "joppa_history", json!({})).await?;
    let p100 = ok(&w, "joppa_history", json!({"limit": 100})).await?;
    let p500 = ok(&w, "joppa_history", json!({"limit": 500})).await?;
    ensure!(
        p25["items"].as_array().map(Vec::len) == Some(25) && p25["next_cursor"].is_string(),
        "{p25}"
    );
    ensure!(
        p100["items"].as_array().map(Vec::len) == Some(100) && p100["next_cursor"].is_string(),
        "{p100}"
    );
    ensure!(
        p500["items"].as_array().map(Vec::len) == Some(100) && p500["next_cursor"].is_string(),
        "{p500}"
    );
    Ok(())
}

// ac:7b8q9bq5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn group_by_without_filter_counts_all_levels_while_count_keeps_roots() -> Result<()> {
    let w = record_world::world(&[]).await?;
    // Archive fixture nodes requirement:r and ac:a so the workspace starts empty, then add 2 domains, 5 capabilities, 12 requirements.
    let a_rev = w.object("ac:a").await?["revision"].clone();
    w.command(
        &w.bot,
        json!({"op": "archive_object", "target": "ac:a", "revision": a_rev, "reason": "Clean fixture"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let r_rev = w.object("requirement:r").await?["revision"].clone();
    w.command(
        &w.bot,
        json!({"op": "archive_object", "target": "requirement:r", "revision": r_rev, "reason": "Clean fixture"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    let tree = ok(&w, "joppa_tree", json!({})).await?;
    w.command(
        &w.owner,
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Configure domain, capability, requirement",
            "put_types": {
                "domain": {"title": "Domain", "role": "group", "status": "children", "root": true, "parents": []},
                "capability": {"title": "Capability", "role": "group", "status": "children", "parents": ["domain"]},
                "requirement": {"title": "Requirement", "role": "requirement", "status": "children", "parents": ["capability"]}
            }
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    let mut ops = vec![];
    for d in 0..2 {
        ops.push(json!({"command": {"op": "save_object", "kind": "domain", "id": format!("d{d}"), "revision": 0, "parent": null, "data": {"title": format!("D{d}"), "owner": "owner"}, "reason": "Seed"}}));
    }
    for c in 0..5 {
        ops.push(json!({"command": {"op": "save_object", "kind": "capability", "id": format!("c{c}"), "revision": 0, "parent": format!("domain:d{}", c % 2), "data": {"title": format!("C{c}"), "owner": "owner"}, "reason": "Seed"}}));
    }
    for r in 0..12 {
        ops.push(json!({"command": {"op": "save_object", "kind": "requirement", "id": format!("r{r}"), "revision": 0, "parent": format!("capability:c{}", r % 5), "data": {"title": format!("R{r}"), "owner": "owner", "body": "Rule"}, "reason": "Seed"}}));
    }
    ok(
        &w,
        "joppa_batch",
        json!({"request_id": "seed-19-nodes", "operations": ops}),
    )
    .await?;

    let grouped = ok(&w, "joppa_objects", json!({"group_by": ["kind"]})).await?;
    ensure!(
        grouped
            == json!({
                "total": 19,
                "groups": [
                    {"kind": "capability", "count": 5},
                    {"kind": "domain", "count": 2},
                    {"kind": "requirement", "count": 12}
                ]
            }),
        "{grouped}"
    );

    let root_count = ok(&w, "joppa_objects", json!({"count": true})).await?;
    ensure!(root_count == json!({"total": 2}), "{root_count}");
    Ok(())
}
