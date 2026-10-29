//! requirement:read-contract over a real server: the joppa_tests sweep over one page of 100
//! and the links of a detail read in pages of 25.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

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
async fn a_detail_read_pages_its_links_by_25() -> Result<()> {
    let w = record_world::world(&[]).await?;
    save_all(&w, "read-pages-targets", 30, |i| {
        json!({"op":"save_object","kind":"requirement","id":format!("t{i:02}"),"revision":0,
            "parent":null,"data":{"title":format!("T{i:02}"),"owner":"owner","body":"Target"},
            "reason":"Link target"})
    })
    .await?;
    let links: Vec<Value> = (0..30)
        .map(|i| json!({"target":format!("requirement:t{i:02}"),"relation":"related"}))
        .collect();
    w.command(
        &w.bot,
        json!({"op":"save_object","kind":"requirement","id":"d","revision":0,
        "parent":null,"data":{"title":"D","owner":"owner","body":"Rule"},"links":links,
        "reason":"Link test"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let detail = read(&w, "joppa_objects", json!({"address":"requirement:d"})).await?;
    let page = &detail["links"];
    let first = page["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        page["total"] == 30
            && page["next_cursor"] == "25"
            && first.len() == 25
            && first.iter().all(|l| l["title"].is_string()),
        "{page}"
    );
    // A titled link gives what the target is now; document.links holds the stored link.
    ensure!(
        first[0]
            == json!({"target":"requirement:t00","relation":"related","title":"T00","current_revision":1}),
        "{}",
        first[0]
    );
    ensure!(
        detail["link_groups"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|g| g["direction"] == "incoming"),
        "{detail}"
    );
    ensure!(
        detail["document"]["links"].as_array().map(Vec::len) == Some(30),
        "document.links is not complete: {}",
        detail["document"]
    );
    let next = read(
        &w,
        "joppa_objects",
        json!({"address":"requirement:d","links_cursor":"25"}),
    )
    .await?;
    let rest = &next["links"];
    ensure!(
        rest["items"].as_array().map(Vec::len) == Some(5)
            && rest["total"] == 30
            && rest.get("next_cursor").is_none(),
        "{rest}"
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
    Ok(())
}

// read-list-fields
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn plain_and_fields_pages_share_address_cursors() -> Result<()> {
    let w = record_world::world(&[]).await?;
    save_all(&w, "read-pages-sweep", 30, |i| {
        json!({"op":"save_object","kind":"ac","id":format!("b{i:02}"),"revision":0,
            "parent":"requirement:r","data":{"title":format!("B{i:02}"),"owner":"owner"},
            "reason":"Sweep test"})
    })
    .await?;
    let plain = read(&w, "joppa_objects", json!({"parent":"requirement:r"})).await?;
    let first = addresses(&plain);
    ensure!(
        first.len() == 25 && plain["next_cursor"] == first[24].as_str(),
        "{plain}"
    );
    // A page with fields continues the plain sweep, from the same snapshot position.
    let named = read(
        &w,
        "joppa_objects",
        json!({"parent":"requirement:r","fields":["revision"],"cursor":plain["next_cursor"]}),
    )
    .await?;
    let rest = addresses(&named);
    ensure!(
        rest.len() == 6
            && rest[0] > first[24]
            && named["items"][0]["revision"] == 1
            && named.get("next_cursor").is_none()
            && plain["position"].is_number()
            && named["position"] == plain["position"]
            && named["workspace"] == plain["workspace"],
        "{named}"
    );
    // And the other way: a fields page gives a bare address that a plain page takes.
    let named = read(
        &w,
        "joppa_objects",
        json!({"parent":"requirement:r","fields":["revision"]}),
    )
    .await?;
    let cursor = named["next_cursor"].as_str().unwrap_or_default().to_owned();
    ensure!(cursor == first[24], "{named}");
    let plain = read(
        &w,
        "joppa_objects",
        json!({"parent":"requirement:r","cursor":cursor}),
    )
    .await?;
    ensure!(addresses(&plain) == rest, "{plain}");
    // The position belongs to the snapshot: a detail read without a write between gives the
    // same position, and a write moves it.
    let detail = read(&w, "joppa_objects", json!({"address":"ac:b00"})).await?;
    ensure!(detail["position"] == plain["position"], "{detail} {plain}");
    w.command(
        &w.bot,
        json!({"op":"save_object","kind":"ac","id":"b99","revision":0,"parent":"requirement:r",
            "data":{"title":"B99","owner":"owner"},"reason":"Sweep test"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let moved = read(&w, "joppa_objects", json!({"parent":"requirement:r"})).await?;
    ensure!(
        moved["position"].as_i64() > plain["position"].as_i64(),
        "{moved} {plain}"
    );
    // A cursor that is not an address is refused, not read from the start.
    for args in [
        json!({"parent":"requirement:r","cursor":"25"}),
        json!({"parent":"requirement:r","fields":["revision"],"cursor":"25"}),
    ] {
        let refused = w.mcp(&w.bot, "joppa_objects", args).await?;
        ensure!(
            refused
                .err()
                .unwrap_or_default()
                .contains("invalid object cursor"),
            "the cursor 25 passed"
        );
    }
    Ok(())
}
