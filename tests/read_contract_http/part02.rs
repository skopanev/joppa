// Part 2 of tests/read_contract_http.rs, included there, so the test names stay unchanged.

// read-detail-pages
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_detail_read_keeps_the_document_and_pages_comments_by_25() -> Result<()> {
    let w = record_world::world(&[]).await?;
    w.command(&w.bot, json!({"op":"save_object","kind":"requirement","id":"d","revision":0,"parent":null,"data":{"title":"D","owner":"owner","body":"Rule","note":null,"projects":[]},"reason":"Detail test"})).await?.map_err(anyhow::Error::msg)?;
    w.command(&w.bot, json!({"op":"save_object","kind":"requirement","id":"d","revision":1,"parent":null,"data":{"title":"D2","owner":"owner","body":"Rule","note":null,"projects":[]},"reason":"Second revision"})).await?.map_err(anyhow::Error::msg)?;
    let notes: Vec<Value> = (0..30)
        .map(|i| json!({"command":{"op":"note","target":"requirement:d","id":format!("c{i:02}"),"text":format!("Comment {i}")}}))
        .collect();
    w.mcp(
        &w.bot,
        "joppa_batch",
        json!({"request_id":"read-contract-notes","operations":notes}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let detail = read(&w, "joppa_objects", json!({"address":"requirement:d"})).await?;
    ensure!(
        detail["data"]
            == json!({"title":"D2","owner":"owner","body":"Rule","note":null,"projects":[]}),
        "the document changed: {}",
        detail["data"]
    );
    ensure!(
        detail["data"]["note"].is_null() && detail["data"]["projects"] == json!([]),
        "{detail}"
    );
    ensure!(
        detail["comments"]["total"] == 30
            && detail["comments"]["items"].as_array().map(Vec::len) == Some(25),
        "{}",
        detail["comments"]
    );
    let comment = &detail["comments"]["items"][0];
    ensure!(
        comment["id"] == "c00"
            && comment["actor"] == "bot"
            && comment.get("author").is_none()
            && comment.get("created_at").is_none(),
        "{comment}"
    );
    // The detail repeats no key of the document or the request, and gives no revision header.
    for key in [
        "workspace",
        "position",
        "address",
        "document",
        "current_revision",
        "parent",
        "present",
        "actor",
        "at",
        "reason",
        "work_ready",
        "revisions",
        "contract_revision",
        "revision_count",
        "kind",
        "id",
        "historical",
        "metadata_scope",
        "workspace_key",
    ] {
        ensure!(detail.get(key).is_none(), "{key}: {detail}");
    }
    ensure!(detail["revision"] == 2, "{detail}");
    let history = read(
        &w,
        "joppa_objects",
        json!({"address":"requirement:d","history":true}),
    )
    .await?;
    let revisions = history["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        history["current_revision"] == 2
            && history["total"] == 2
            && history.get("revisions").is_none()
            && revisions.len() == 2
            && revisions[1]["revision"] == 2
            && revisions[1]["reason"] == "Second revision"
            && revisions[1]["actor"] == "bot",
        "{history}"
    );
    let third = read(
        &w,
        "joppa_objects",
        json!({"address":"requirement:d","comments_cursor":"25"}),
    )
    .await?;
    ensure!(
        third["comments"]["items"].as_array().map(Vec::len) == Some(5)
            && third["comments"].get("next_cursor").is_none(),
        "{}",
        third["comments"]
    );
    let old = read(
        &w,
        "joppa_objects",
        json!({"address":"requirement:d","revision":1}),
    )
    .await?;
    ensure!(
        old["document"]["data"]["title"] == "D"
            && old["historical"] == true
            && old["current_revision"] == 2,
        "{old}"
    );
    ensure!(
        no_empty_keys(&detail),
        "an empty key outside document: {detail}"
    );
    Ok(())
}

// read-empty-verbose-tests
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn empty_keys_and_item_ids_go_and_tests_read_by_kind() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    save_ac(&w, "b").await?;
    let compact = read(&w, "joppa_objects", json!({"address":"ac:a"})).await?;
    ensure!(no_empty_keys(&compact), "{compact}");
    // No item ID, no signal copy of status, and no empty comment or link page.
    for key in ["item_id", "signal", "comments", "links"] {
        ensure!(compact.get(key).is_none(), "{key}: {compact}");
    }
    ensure!(
        compact["annotations"].get("attachments").is_none() && compact["status"].is_object(),
        "{compact}"
    );
    // No copy of the address or the parent in process, and no remote link list without
    // remote links. Its revision 0 goes too: set_remote_links takes a missing one as 0.
    let process = &compact["process"];
    ensure!(
        process.get("annotation_address").is_none()
            && process.get("criterion").is_none()
            && compact["parent"] == "requirement:r"
            && process.get("req").is_none()
            && process["assessment"].get("criterion").is_none()
            && process["assessment"].get("req").is_none(),
        "{process}"
    );
    for key in ["remote_links", "remote_review_required"] {
        ensure!(compact.get(key).is_none(), "{key}: {compact}");
    }
    ensure!(compact.get("remote_links_revision").is_none(), "{compact}");
    let verbose = w
        .mcp(
            &w.bot,
            "joppa_objects",
            json!({"address":"ac:a","verbose":true}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        verbose.get("signal").is_none(),
        "verbose still works: {verbose}"
    );
    let empty = read(&w, "joppa_objects", json!({"parent":"ac:a"})).await?;
    ensure!(empty["items"] == json!([]), "{empty}");
    let swept = read(&w, "joppa_tests", json!({"kind":"ac","parent":"*"})).await?;
    let addresses: Vec<&str> = swept["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["address"].as_str())
        .collect();
    ensure!(
        swept["total"] == 2 && addresses == ["ac:a", "ac:b"],
        "{swept}"
    );
    let many = (0..101)
        .map(|i| format!("ac:x{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let refused = w.mcp(&w.bot, "joppa_tests", json!({"ac":many})).await?;
    ensure!(
        refused
            .err()
            .unwrap_or_default()
            .contains("at most 100 criteria in one call"),
        "101 criteria passed"
    );
    Ok(())
}
