//! requirement:read-contract over a real server: every MCP list read gives one page shape,
//! `{items, total, next_cursor}`, at most 25 items, and takes `cursor` for the next page.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

async fn read(w: &record_world::World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args).await?.map_err(anyhow::Error::msg)
}

fn count(reply: &Value) -> usize {
    reply["items"].as_array().map_or(0, Vec::len)
}

/// 30 criteria `ac:b00` ... `ac:b29` titled "Paging item NN", and 28 more revisions of `ac:a`
/// that link to all of them.
async fn fixture(w: &record_world::World) -> Result<()> {
    let mut operations: Vec<Value> = (0..30)
        .map(|i| {
            json!({"command":{"op":"save_object","kind":"ac","id":format!("b{i:02}"),"revision":0,
            "parent":"requirement:r","data":{"title":format!("Paging item {i:02}"),"owner":"owner"},
            "reason":"Paging test"}})
        })
        .collect();
    let links: Vec<Value> = (0..30)
        .map(|i| json!({"target":format!("ac:b{i:02}"),"relation":"related"}))
        .collect();
    for revision in 2..30 {
        operations.push(json!({"command":{"op":"save_object","kind":"ac","id":"a","revision":revision,
            "parent":"requirement:r","links":links,
            "data":{"title":"A","owner":"owner","tests":["tests/x.rs > t"],"body":format!("Revision {}", revision + 1)},
            "reason":format!("Revision {}", revision + 1)}}));
    }
    w.mcp(
        &w.bot,
        "joppa_batch",
        json!({"request_id":"mcp-pages","operations":operations}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    Ok(())
}

// read-mcp-pages
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn every_mcp_list_gives_items_total_and_next_cursor() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    fixture(&w).await?;
    // Revision headers: 30 revisions of ac:a give 25, then 5.
    let first = read(
        &w,
        "joppa_objects",
        json!({"address":"ac:a","history":true}),
    )
    .await?;
    let rest = read(
        &w,
        "joppa_objects",
        json!({"address":"ac:a","history":true,"cursor":"25"}),
    )
    .await?;
    ensure!(
        count(&first) == 25
            && first["total"] == 30
            && first["next_cursor"] == "25"
            && first["items"][0]["revision"] == 1
            && first.get("revisions").is_none()
            && count(&rest) == 5
            && rest.get("next_cursor").is_none(),
        "{first} {rest}"
    );
    // One link group: 30 links give 25 with the total of the group.
    let links = read(&w, "joppa_objects", json!({"links_of":"ac:a","direction":"outgoing","relation":"related","kind":"ac","limit":100})).await?;
    ensure!(
        count(&links) == 25
            && links["total"] == 30
            && links["next_cursor"] == "outgoing/related/ac:b24",
        "{links}"
    );
    // Search: a larger limit gives 25, and the cursor gives the rest of the same ranking.
    let found = read(
        &w,
        "joppa_search",
        json!({"query":"paging","kind":"ac","limit":100}),
    )
    .await?;
    let more = read(
        &w,
        "joppa_search",
        json!({"query":"paging","kind":"ac","limit":100,"cursor":found["next_cursor"]}),
    )
    .await?;
    ensure!(
        count(&found) == 25
            && found["total"] == 30
            && found["next_cursor"] == "25"
            && count(&more) == 5
            && more.get("next_cursor").is_none(),
        "{found} {more}"
    );
    let hit = &found["items"][0];
    ensure!(
        hit.get("system_id").is_none()
            && hit.get("workspace").is_none()
            && hit["req"] == "requirement:r",
        "{hit}"
    );
    let requirement = read(
        &w,
        "joppa_search",
        json!({"query":"The rule","kind":"requirement"}),
    )
    .await?;
    ensure!(
        requirement["items"][0]["address"] == "requirement:r"
            && requirement["items"][0].get("req").is_none(),
        "{requirement}"
    );
    // Timeline and History: 25 by default and for a larger limit, with the total.
    let timeline = read(&w, "joppa_timeline", json!({"limit":100})).await?;
    let history = read(&w, "joppa_history", json!({})).await?;
    let all = read(&w, "joppa_history", json!({"all":true,"limit":100})).await?;
    ensure!(
        count(&timeline) == 25
            && timeline["total"].as_u64() > Some(25)
            && timeline["next_cursor"].is_string(),
        "{timeline}"
    );
    ensure!(
        count(&history) == 25
            && history["total"].as_u64() > Some(25)
            && count(&all) == 25
            && all["total"] == timeline["total"],
        "{history} {all}"
    );
    // Tasks: `items`, not `tasks`, with the total.
    let tasks = read(&w, "joppa_tasks", json!({"view":"all","limit":100})).await?;
    ensure!(
        tasks["items"].is_array() && tasks.get("tasks").is_none() && tasks["total"] == 0,
        "{tasks}"
    );
    // Workspaces and the test overview.
    // The test database can hold the workspaces of earlier runs, so the check does not
    // count them. It follows the pages until it finds this workspace; each page has at most
    // 25 items and the same total.
    let mut cursor = Value::Null;
    let mut seen = 0;
    let found = loop {
        let args = if cursor.is_null() {
            json!({})
        } else {
            json!({ "cursor": cursor })
        };
        let listed = read(&w, "joppa_list", args).await?;
        seen += count(&listed);
        ensure!(
            (1..=25).contains(&count(&listed)) && listed["total"].as_u64() >= Some(seen as u64),
            "{listed}"
        );
        if listed["items"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|i| i["id"] == w.ws.as_str() && i["title"] == "Record channel")
        {
            break true;
        }
        match &listed["next_cursor"] {
            Value::String(next) => cursor = json!(next),
            _ => break false,
        }
    };
    ensure!(found, "joppa_list has no {}", w.ws);
    w.recorded(
        &"a".repeat(40),
        json!([{"test":"tests/x.rs > t","status":"pass"}]),
    )
    .await?;
    let overview = read(&w, "joppa_tests", json!({})).await?;
    ensure!(
        overview["total"] == 1
            && overview["items"][0]
                == json!({"address":"ac:a","status":"green","ancestors":["requirement:r"]}),
        "{overview}"
    );
    Ok(())
}
