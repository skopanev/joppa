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
    w.command(&w.bot, json!({"op":"save_object","kind":"ac","id":"a1","revision":0,"parent":"requirement:r","data":{"title":"A1","owner":"owner","assignee":"lane3"},"reason":"Read contract test"})).await?.map_err(anyhow::Error::msg)?;
    for i in 2..27 {
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
    // A plain item gives only address, title, the status tone and assignee when set. No plain
    // item gives children or child_counts.
    for item in &items {
        let keys: Vec<&String> = item
            .as_object()
            .map(|m| m.keys().collect())
            .unwrap_or_default();
        ensure!(
            keys.iter()
                .all(|k| ["address", "title", "status", "assignee"].contains(&k.as_str()))
                && item["status"].is_string()
                && (item.get("assignee").is_some() == (item["address"] == "ac:a1")),
            "{item}"
        );
    }
    let a1 = items.iter().find(|i| i["address"] == "ac:a1");
    ensure!(a1.is_some_and(|i| i["assignee"] == "lane3"), "{first}");
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
    // With fields, limit 100 gives all 27 items. status is the same tone as in the plain item
    // and in the detail read.
    let args = json!({"parent":"requirement:r","limit":100,"fields":["status","children"]});
    let named = read(&w, "joppa_objects", args).await?;
    let all = named["items"].as_array().cloned().unwrap_or_default();
    ensure!(all.len() == 27 && named["total"] == 27, "{named}");
    for item in &items {
        let same = all.iter().find(|i| i["address"] == item["address"]);
        ensure!(same.is_some_and(|i| i["status"] == item["status"]), "{item} {named}");
    }
    let detail = read(&w, "joppa_objects", json!({"address":"ac:a"})).await?;
    let a = all.iter().find(|i| i["address"] == "ac:a");
    ensure!(
        a.is_some_and(|i| i["status"] == detail["status"]["tone"]),
        "{detail} {named}"
    );
    Ok(())
}

// read-list-fields
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn fields_return_each_named_path_and_null_for_a_name_without_data() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let reply = read(&w, "joppa_objects", json!({"parent":"requirement:r","fields":["revision","process.verification","work_ready"]})).await?;
    let item = &reply["items"][0];
    ensure!(
        item["address"] == "ac:a"
            && item["revision"].is_number()
            && item.get("process.verification") == Some(&Value::Null)
            && item["work_ready"].is_boolean()
            && item.get("title").is_none(),
        "{reply}"
    );
    // data and data.<key> give the stored document data, without a cut.
    let long = "Given: a long text. ".repeat(30);
    w.command(&w.bot, json!({"op":"save_object","kind":"ac","id":"long","revision":0,"parent":"requirement:r","data":{"title":"Long","owner":"owner","body":long},"reason":"Read contract test"})).await?.map_err(anyhow::Error::msg)?;
    let full = read(
        &w,
        "joppa_objects",
        json!({"parent":"requirement:r","fields":["data.body","data"]}),
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
        saved["data.body"] == json!(long)
            && saved["data"]["body"] == json!(long)
            && saved["data"]["title"] == "Long"
            && full.get("position").is_none()
            && full.get("workspace").is_none(),
        "{full}"
    );
    // A bare name that no document holds gives null on each item.
    let nope = json!({"parent":"requirement:r","fields":["nope"]});
    let nope = read(&w, "joppa_objects", nope).await?;
    let items = nope["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        items.len() == 2
            && items.iter().all(|i| i.get("nope") == Some(&Value::Null)
                && i.as_object().map(|m| m.len()) == Some(2)),
        "{nope}"
    );
    Ok(())
}
