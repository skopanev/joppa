// Part 1 of tests/read_pages_http.rs, included there, so the test names stay unchanged.

async fn read(w: &record_world::World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args).await?.map_err(anyhow::Error::msg)
}

/// One joppa_batch of save_object operations, each from `save(i)`.
async fn save_all(
    w: &record_world::World,
    request: &str,
    count: usize,
    save: impl Fn(usize) -> Value,
) -> Result<()> {
    let operations: Vec<Value> = (0..count).map(|i| json!({"command": save(i)})).collect();
    w.mcp(
        &w.bot,
        "joppa_batch",
        json!({"request_id":request,"operations":operations}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    Ok(())
}

fn addresses(reply: &Value) -> Vec<String> {
    reply["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["address"].as_str().map(str::to_owned))
        .collect()
}

// read-tests-sweep
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_tests_sweep_of_101_criteria_gives_100_then_1() -> Result<()> {
    let w = record_world::world(&[]).await?;
    save_all(&w, "read-pages-acs", 100, |i| {
        json!({"op":"save_object","kind":"ac","id":format!("b{i:03}"),"revision":0,
            "parent":"requirement:r","data":{"title":format!("B{i:03}"),"owner":"owner"},
            "reason":"Sweep test"})
    })
    .await?;
    let first = read(&w, "joppa_tests", json!({"kind":"ac","parent":"*"})).await?;
    let first_page = addresses(&first);
    ensure!(
        first_page.len() == 100
            && first["total"] == 101
            && first["next_cursor"] == "100"
            && first_page[0] == "ac:a"
            && first_page[99] == "ac:b098",
        "{first}"
    );
    let second = read(
        &w,
        "joppa_tests",
        json!({"kind":"ac","parent":"*","cursor":"100"}),
    )
    .await?;
    ensure!(
        addresses(&second) == ["ac:b099"]
            && second["total"] == 101
            && second.get("next_cursor").is_none(),
        "{second}"
    );
    Ok(())
}

// read-detail-links
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_detail_read_gives_all_its_links_in_one_list() -> Result<()> {
    let w = record_world::world(&[]).await?;
    save_all(&w, "read-pages-targets", 30, |i| {
        json!({"op":"save_object","kind":"requirement","id":format!("t{i:02}"),"revision":0,
            "parent":null,"data":{"title":format!("T{i:02}"),"owner":"owner","body":"Target"},
            "reason":"Link target"})
    })
    .await?;
    // 29 related links and one blocking dependency with a pin.
    let mut links: Vec<Value> = (0..29)
        .map(|i| json!({"target":format!("requirement:t{i:02}"),"relation":"related"}))
        .collect();
    links.push(json!({"target":"requirement:t29","relation":"depends_on","revision":1,"blocking":true}));
    w.command(
        &w.bot,
        json!({"op":"save_object","kind":"requirement","id":"d","revision":0,
        "parent":null,"data":{"title":"D","owner":"owner","body":"Rule"},"links":links,
        "reason":"Link test"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let detail = read(&w, "joppa_objects", json!({"address":"requirement:d"})).await?;
    let items = detail["links"].as_array().cloned().unwrap_or_default();
    ensure!(
        items.len() == 30 && items.iter().all(|l| l["title"].is_string()),
        "{detail}"
    );
    // One list: the stored link with the title of its target. A related link has no
    // relation and no pin.
    ensure!(
        items[0] == json!({"target":"requirement:t00","title":"T00"})
            && items[29]
                == json!({"target":"requirement:t29","relation":"depends_on","title":"T29",
                    "revision":1,"current_revision":1,"blocking":true}),
        "{} {}",
        items[0],
        items[29]
    );
    ensure!(
        detail["link_groups"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|g| g["direction"] == "incoming")
            && detail.get("document").is_none(),
        "{detail}"
    );
    // save_object takes the read links back as they are and stores the same links.
    w.command(
        &w.bot,
        json!({"op":"save_object","kind":"requirement","id":"d","revision":detail["revision"],
        "parent":null,"data":{"title":"D","owner":"owner","body":"Rule v2"},"links":items,
        "reason":"Save the read links"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let saved = read(
        &w,
        "joppa_objects",
        json!({"address":"requirement:d","revision":2}),
    )
    .await?;
    // Revision 2 holds the same links, so its read gives the same items.
    ensure!(saved["links"] == json!(items), "{saved}");
    Ok(())
}

// read-list-compact
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_list_item_shows_the_same_tone_as_the_detail_read() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    w.recorded(
        &"a".repeat(40),
        json!([{"test":"tests/x.rs > t","status":"pass"}]),
    )
    .await?;
    let list = read(&w, "joppa_objects", json!({"parent":"requirement:r"})).await?;
    let detail = read(&w, "joppa_objects", json!({"address":"ac:a"})).await?;
    let named = read(
        &w,
        "joppa_objects",
        json!({"parent":"requirement:r","fields":["status"]}),
    )
    .await?;
    ensure!(
        detail["status"]["tone"] == "green"
            && list["items"][0]["status"] == "green"
            && named["items"][0]["status"] == "green",
        "list {list} fields {named} detail {}",
        detail["status"]
    );
    // The requirement is green by its children, and its label says so.
    let req = read(&w, "joppa_objects", json!({"address":"requirement:r"})).await?;
    ensure!(
        req["status"] == json!({"label":"Children checks passed","tone":"green"}),
        "{}",
        req["status"]
    );
    Ok(())
}
