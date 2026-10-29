//! requirement:read-contract over a real server: lists, fields, details, empty keys,
//! no item IDs and joppa_tests by kind.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

async fn read(w: &record_world::World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args).await?.map_err(anyhow::Error::msg)
}

async fn save_ac(w: &record_world::World, id: &str) -> Result<()> {
    let saved = w.command(&w.bot, json!({"op":"save_object","kind":"ac","id":id,"revision":0,"parent":"requirement:r","data":{"title":id.to_uppercase(),"owner":"owner"},"reason":"Read contract test"})).await?;
    saved.map(|_| ()).map_err(anyhow::Error::msg)
}

/// True when no key outside document, data, command or remote_links holds null, "", [] or {}.
fn no_empty_keys(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().all(|(k, v)| {
            ["document", "data", "command", "remote_links"].contains(&k.as_str())
                || (!matches!(v, Value::Null)
                    && v != &json!("")
                    && v != &json!([])
                    && v != &json!({})
                    && no_empty_keys(v))
        }),
        Value::Array(items) => items.iter().all(no_empty_keys),
        _ => true,
    }
}

// read-list-compact
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_list_page_holds_at_most_25_compact_items() -> Result<()> {
    let w = record_world::world(&[]).await?;
    for i in 1..27 {
        save_ac(&w, &format!("a{i}")).await?;
    }
    let first = read(
        &w,
        "joppa_objects",
        json!({"parent":"requirement:r","limit":100}),
    )
    .await?;
    let items = first["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        items.len() == 25
            && first["total"] == 27
            && first["position"].is_number()
            && first["workspace"].is_string(),
        "{first}"
    );
    for item in &items {
        let keys: Vec<&String> = item
            .as_object()
            .map(|m| m.keys().collect())
            .unwrap_or_default();
        ensure!(
            keys.iter()
                .all(|k| ["address", "title", "status", "children"].contains(&k.as_str())),
            "{item}"
        );
    }
    ensure!(
        serde_json::to_string(&first["items"])?.len() < 25 * 64,
        "{first}"
    );
    let order: Vec<&str> = items.iter().filter_map(|i| i["address"].as_str()).collect();
    let mut sorted = order.clone();
    sorted.sort_unstable();
    ensure!(
        order == sorted && first["next_cursor"].as_str() == order.last().copied(),
        "{first}"
    );
    let cursor = first["next_cursor"].as_str().unwrap_or_default().to_owned();
    let second = read(
        &w,
        "joppa_objects",
        json!({"parent":"requirement:r","cursor":cursor}),
    )
    .await?;
    ensure!(
        second["items"].as_array().map(Vec::len) == Some(2),
        "{second}"
    );
    ensure!(second.get("next_cursor").is_none(), "{second}");
    Ok(())
}

// read-list-fields
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn fields_return_each_named_path_and_refuse_an_unknown_one() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let reply = read(&w, "joppa_objects", json!({"parent":"requirement:r","fields":["revision","process.planning_claim","work_ready"]})).await?;
    let item = &reply["items"][0];
    ensure!(
        item["address"] == "ac:a"
            && item["revision"].is_number()
            && item.get("process.planning_claim") == Some(&Value::Null)
            && item["work_ready"].is_boolean()
            && item.get("title").is_none(),
        "{reply}"
    );
    // data and data.<key> give the stored document data, without a cut.
    let long = "Given: a long text. ".repeat(30);
    w.command(&w.bot, json!({"op":"save_object","kind":"ac","id":"long","revision":0,"parent":"requirement:r","data":{"title":"Long","owner":"owner","description":long},"reason":"Read contract test"})).await?.map_err(anyhow::Error::msg)?;
    let full = read(
        &w,
        "joppa_objects",
        json!({"parent":"requirement:r","fields":["data.description","data"]}),
    )
    .await?;
    let saved = full["items"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|i| i["address"] == "ac:long")
        .cloned()
        .unwrap_or_default();
    ensure!(
        saved["data.description"] == json!(long)
            && saved["data"]["description"] == json!(long)
            && saved["data"]["title"] == "Long"
            && full["position"].is_number()
            && full["workspace"].is_string(),
        "{full}"
    );
    let refused = w
        .mcp(
            &w.bot,
            "joppa_objects",
            json!({"parent":"requirement:r","fields":["nope"]}),
        )
        .await?;
    ensure!(
        refused
            .err()
            .unwrap_or_default()
            .contains("unknown field nope"),
        "an unknown field passed"
    );
    Ok(())
}

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
        detail["document"]["data"]
            == json!({"title":"D2","owner":"owner","body":"Rule","note":null,"projects":[]}),
        "the document changed: {}",
        detail["document"]
    );
    ensure!(
        detail["document"]["data"]["note"].is_null()
            && detail["document"]["data"]["projects"] == json!([]),
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
    // The detail repeats no key of the document or the request; position and workspace stay.
    for key in [
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
    ensure!(
        detail["document"]["revision"] == 2
            && detail["current_revision"] == 2
            && detail["position"].is_number()
            && detail["workspace"].is_string(),
        "{detail}"
    );
    let history = read(
        &w,
        "joppa_objects",
        json!({"address":"requirement:d","history":true}),
    )
    .await?;
    let revisions = history["revisions"].as_array().cloned().unwrap_or_default();
    ensure!(
        history["current_revision"] == 2
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
            && compact["document"]["parent"] == "requirement:r"
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
