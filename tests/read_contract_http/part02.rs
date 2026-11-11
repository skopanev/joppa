// Part 2 of tests/read_contract_http.rs, included there, so the test names stay unchanged.

// read-detail-pages
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_detail_read_keeps_the_document_and_gives_a_comment_count() -> Result<()> {
    let w = record_world::world(&[]).await?;
    w.command(&w.bot, json!({"op":"save_object","kind":"requirement","id":"d","revision":0,"parent":null,"data":{"title":"D","owner":"owner","body":"Rule","note":null,"projects":[]},"reason":"Detail test"})).await?.map_err(anyhow::Error::msg)?;
    // clean: true keeps the sent document whole, null values included.
    w.command(&w.bot, json!({"op":"save_object","kind":"requirement","id":"d","revision":1,"parent":null,"clean":true,"data":{"title":"D2","owner":"owner","body":"Rule","note":null,"projects":[]},"reason":"Second revision"})).await?.map_err(anyhow::Error::msg)?;
    comments(&w, "requirement:d", 30, true).await?;
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
    // The card counts the comments without the withdrawn one, and gives no comment page.
    ensure!(
        detail["comments_count"] == 30 && detail.get("comments").is_none(),
        "{detail}"
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
    let old = read(
        &w,
        "joppa_objects",
        json!({"address":"requirement:d","revision":1}),
    )
    .await?;
    ensure!(
        old["data"]["title"] == "D"
            && old["revision"] == 1
            && old.get("historical").is_none()
            && old.get("document").is_none()
            && old["current_revision"] == 2,
        "{old}"
    );
    ensure!(
        no_empty_keys(&detail),
        "an empty key outside document: {detail}"
    );
    // A detail drops work_ready: true, process.confirmed: false and the zero counts.
    let r = read(&w, "joppa_objects", json!({"address":"requirement:r"})).await?;
    ensure!(
        r.get("comments_count").is_none()
            && r.get("files_count").is_none()
            && r.get("work_ready") != Some(&json!(true))
            && r["process"].get("confirmed") != Some(&json!(false)),
        "{r}"
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
    // No item ID, no signal copy of status, no empty comment page and no empty relations.
    // No read gives links.
    for key in ["item_id", "signal", "comments", "links", "relations"] {
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
            && process.get("req").is_none(),
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
    // Criterion B has no child, so its list is empty.
    let empty = read(&w, "joppa_objects", json!({"parent":"ac:b"})).await?;
    ensure!(empty["items"] == json!([]), "{empty}");
    // A read by kind gives the criteria of R, and the test node of A.
    let swept = read(&w, "joppa_objects", json!({"kind":"ac","parent":"*"})).await?;
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
    let tests = read(&w, "joppa_objects", json!({"parent":"ac:a","kind":"test"})).await?;
    let items = tests["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        tests["total"] == 1
            && items.len() == 1
            && items[0]["title"] == "tests/x.rs > t"
            && items[0]["address"]
                .as_str()
                .is_some_and(|a| a.starts_with("test:")),
        "{tests}"
    );
    Ok(())
}
