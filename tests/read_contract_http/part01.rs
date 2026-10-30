// Part 1 of tests/read_contract_http.rs, included there, so the test names stay unchanged.

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
            && first.get("position").is_none()
            && first.get("workspace").is_none(),
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
            && full.get("position").is_none()
            && full.get("workspace").is_none(),
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
