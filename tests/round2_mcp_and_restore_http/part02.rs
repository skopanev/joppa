// Part 2 of tests/round2_mcp_and_restore_http.rs: ac:m3c0yv7q and ac:yg6tawf1.

// ac:m3c0yv7q (nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn objects_fields_description_names_revision_and_list_returns_revisions_for_save()
-> Result<()> {
    let w = record_world::world(&[]).await?;
    let tools = tools_list(&w).await?;
    let tool = tools
        .iter()
        .find(|t| t["name"] == "joppa_objects")
        .context("missing joppa_objects")?;
    let desc = tool["inputSchema"]["properties"]["fields"]["description"]
        .as_str()
        .unwrap_or_default();
    for word in ["title", "revision", "body", "status"] {
        ensure!(desc.contains(word), "fields description missing {word}: {desc}");
    }

    let list = mcp_ok(
        &w,
        "joppa_objects",
        json!({"parent": "requirement:r", "fields": ["title", "revision"]}),
    )
    .await?;
    ensure!(
        list["items"] == json!([{"address": "ac:a", "title": "A", "revision": 1}]),
        "{list}"
    );
    let rev = list["items"][0]["revision"].as_u64().unwrap();
    ok(
        &w,
        json!({"op": "save_object", "kind": "ac", "id": "a", "revision": rev, "data": {"title": "A2"}, "reason": "Update with listed revision"}),
    )
    .await?;
    ensure!(w.object("ac:a").await?["revision"] == 2);
    Ok(())
}

// ac:m3c0yv7q (off-nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn objects_fields_still_refuses_removed_or_non_string_field_entries() -> Result<()> {
    let w = record_world::world(&[]).await?;
    refused(
        w.mcp(
            &w.bot,
            "joppa_objects",
            json!({"parent": "requirement:r", "fields": ["revision", "work_ready"]}),
        )
        .await?,
        "work_ready was removed",
    )?;
    refused(
        w.mcp(
            &w.bot,
            "joppa_objects",
            json!({"parent": "requirement:r", "fields": [123]}),
        )
        .await?,
        "fields is a list of key names",
    )?;
    Ok(())
}

// ac:yg6tawf1 (nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn unknown_operation_name_keeps_unknown_operation_refusal() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let before = head(&w).await?;
    refused(
        w.command(&w.bot, json!({"op": "no_such_op"})).await?,
        "unknown operation",
    )?;
    refused(
        batch(
            &w,
            &[
                json!({"op": "save_object", "kind": "ac", "id": "b", "revision": 0, "parent": "requirement:r", "data": {"title": "B", "owner": "owner"}, "reason": "Create"}),
                json!({"op": "no_such_op"}),
            ],
        )
        .await?,
        "operation 2: unknown operation",
    )?;
    ensure!(head(&w).await? == before);
    Ok(())
}

// ac:yg6tawf1 (off-nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn command_without_op_or_with_empty_op_fails_with_missing_field_op() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let before = head(&w).await?;
    for bad in [
        json!({"kind": "ac", "id": "x"}),
        json!({"op": null, "kind": "ac"}),
        json!({"op": "", "kind": "ac"}),
    ] {
        let reply = w.command(&w.bot, bad).await?;
        refused(reply, "missing field `op`")?;
    }
    let batch_reply = batch(
        &w,
        &[
            json!({"op": "save_object", "kind": "ac", "id": "b", "revision": 0, "parent": "requirement:r", "data": {"title": "B", "owner": "owner"}, "reason": "Valid first op"}),
            json!({"kind": "ac", "id": "x"}),
        ],
    )
    .await?;
    refused(batch_reply, "operation 2: missing field `op`")?;
    ensure!(head(&w).await? == before, "failed batch appended events");
    Ok(())
}
