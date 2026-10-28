//! requirement:read-contract over a real server: lists, fields, details, empty keys,
//! the verbose shape and joppa_tests by kind.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

/// record_world asks for the verbose shape unless a call names it: read the compact one.
fn compact_args(mut args: Value) -> Value {
    if args.get("verbose").is_none() {
        args["verbose"] = json!(false);
    }
    args
}

async fn read(w: &record_world::World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, compact_args(args))
        .await?
        .map_err(anyhow::Error::msg)
}

async fn save_ac(w: &record_world::World, id: &str) -> Result<()> {
    let saved = w.command(&w.bot, json!({"op":"save_object","kind":"ac","id":id,"revision":0,"parent":"requirement:r","data":{"title":id.to_uppercase(),"owner":"owner"},"reason":"Read contract test"})).await?;
    saved.map(|_| ()).map_err(anyhow::Error::msg)
}

/// True when no key outside document, data or command holds null, "", [] or {}.
fn no_empty_keys(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().all(|(k, v)| {
            ["document", "data", "command"].contains(&k.as_str())
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
    ensure!(items.len() == 25 && first["total"] == 27, "{first}");
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
    let refused = w
        .mcp(
            &w.bot,
            "joppa_objects",
            compact_args(json!({"parent":"requirement:r","fields":["nope"]})),
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
    let verbose = read(
        &w,
        "joppa_objects",
        json!({"address":"requirement:d","verbose":true}),
    )
    .await?;
    let detail = read(&w, "joppa_objects", json!({"address":"requirement:d"})).await?;
    ensure!(
        detail["document"] == verbose["document"],
        "the document changed:\n{}\n{}",
        detail["document"],
        verbose["document"]
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
    let revisions = detail["revisions"].as_array().cloned().unwrap_or_default();
    ensure!(
        revisions.len() == 2
            && revisions[1]["revision"] == 2
            && revisions[1]["reason"] == "Second revision"
            && revisions[1]["actor"] == "bot",
        "{revisions:?}"
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
    ensure!(old["document"]["data"]["title"] == "D", "{old}");
    ensure!(
        no_empty_keys(&detail),
        "an empty key outside document: {detail}"
    );
    Ok(())
}

// read-empty-verbose-tests
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn empty_keys_go_verbose_keeps_them_and_tests_read_by_kind() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    save_ac(&w, "b").await?;
    let compact = read(&w, "joppa_objects", json!({"address":"ac:a"})).await?;
    let verbose = read(
        &w,
        "joppa_objects",
        json!({"address":"ac:a","verbose":true}),
    )
    .await?;
    ensure!(no_empty_keys(&compact), "{compact}");
    ensure!(
        verbose.get("signal").is_some() && compact.get("signal").is_none(),
        "verbose lost its shape"
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
    let refused = w
        .mcp(&w.bot, "joppa_tests", compact_args(json!({"ac":many})))
        .await?;
    ensure!(
        refused
            .err()
            .unwrap_or_default()
            .contains("at most 100 criteria in one call"),
        "101 criteria passed"
    );
    Ok(())
}
