// Part 1 of tests/read_pages_http.rs, included there, so the test names stay unchanged.

// An included file resolves #[path] from its own folder.
#[path = "../common/relation_edges.rs"]
mod relation_edges;

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
    // 29 related edges and one depends_on edge with a pin.
    let related: Vec<Value> = (0..29)
        .map(|i| json!({"to":format!("requirement:t{i:02}")}))
        .collect();
    let relations = json!({"related":{"edges":related},
        "depends_on":{"edges":[{"to":"requirement:t29","revision":1}]}});
    w.command(
        &w.bot,
        json!({"op":"save_object","kind":"requirement","id":"d","revision":0,
        "parent":null,"data":{"title":"D","owner":"owner","body":"Rule"},"relations":relations,
        "reason":"Link test"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    // requirement:u has a related edge to d: the one incoming edge of d.
    w.command(
        &w.bot,
        json!({"op":"save_object","kind":"requirement","id":"u","revision":0,
        "parent":null,"data":{"title":"U","owner":"owner","body":"Rule"},
        "relations":{"related":{"edges":[{"to":"requirement:d"}]}},"reason":"Link test"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let detail = read(&w, "joppa_objects", json!({"address":"requirement:d"})).await?;
    let items = relation_edges::edges(&detail, "related");
    let pinned = relation_edges::edges(&detail, "depends_on");
    ensure!(
        items.len() == 29 && pinned.len() == 1 && items.iter().all(|e| e["title"].is_string()),
        "{detail}"
    );
    // Each edge gives the title of its target. A related edge without a pin has no revision,
    // and a pin shows the current revision. No edge gives blocking.
    ensure!(
        items[0]["to"] == "requirement:t00"
            && items[0]["title"] == "T00"
            && items[0].get("revision").is_none()
            && pinned[0]["to"] == "requirement:t29"
            && pinned[0]["title"] == "T29"
            && pinned[0]["revision"] == 1
            && pinned[0]["current_revision"] == 1
            && pinned[0].get("blocking").is_none(),
        "{} {}",
        items[0],
        pinned[0]
    );
    // The groups count the incoming edges, with no direction key. The detail gives no
    // references and no child_counts without fields.
    ensure!(
        detail["relation_groups"] == json!([{"relation":"related","kind":"requirement","count":1}]),
        "{detail}"
    );
    for key in ["document", "links", "blocking", "references", "child_counts"] {
        ensure!(detail.get(key).is_none(), "{key}: {detail}");
    }
    // save_object takes the read relations back as they are and stores the same edges.
    w.command(
        &w.bot,
        json!({"op":"save_object","kind":"requirement","id":"d","revision":detail["revision"],
        "parent":null,"data":{"title":"D","owner":"owner","body":"Rule v2"},
        "relations":detail["relations"],"reason":"Save the read relations"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let saved = read(
        &w,
        "joppa_objects",
        json!({"address":"requirement:d","revision":2}),
    )
    .await?;
    // Revision 2 holds the same edges, so its read gives the same edges.
    ensure!(
        relation_edges::edges(&saved, "related") == items
            && relation_edges::edges(&saved, "depends_on") == pinned,
        "{saved}"
    );
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
    // Depth 1 gives the direct children of R: criterion A, without its test node.
    let list = read(&w, "joppa_objects", json!({"parent":"requirement:r","depth":1})).await?;
    let detail = read(&w, "joppa_objects", json!({"address":"ac:a"})).await?;
    let named = read(
        &w,
        "joppa_objects",
        json!({"parent":"requirement:r","depth":1,"fields":["status"]}),
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
