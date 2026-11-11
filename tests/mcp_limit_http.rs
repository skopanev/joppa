//! requirement:read-contract over a real server: the limit of joppa_objects takes 100 when
//! fields is sent, and 25 without fields. Every other MCP list takes a limit of 1 to 25.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

async fn read(w: &record_world::World, args: Value) -> Result<Value> {
    w.mcp(&w.bot, "joppa_objects", args)
        .await?
        .map_err(anyhow::Error::msg)
}

fn count(reply: &Value) -> usize {
    reply["items"].as_array().map_or(0, Vec::len)
}

/// 119 more criteria under requirement:r, so r has 120 criteria with ac:a. Each new criterion
/// has a related edge to requirement:r.
async fn fixture(w: &record_world::World) -> Result<()> {
    let save = |i: usize| {
        json!({"command":{"op":"save_object","kind":"ac","id":format!("c{i:03}"),"revision":0,
        "parent":"requirement:r","data":{"title":format!("C{i:03}"),"owner":"owner"},
        "relations":{"related":{"edges":[{"to":"requirement:r"}]}},"reason":"Limit test"}})
    };
    for (n, range) in [(0, 0..100), (1, 100..119)] {
        let operations: Vec<Value> = range.map(save).collect();
        let batch = json!({"request_id":format!("mcp-limit-{n}"),"operations":operations});
        w.mcp(&w.bot, "joppa_batch", batch)
            .await?
            .map_err(anyhow::Error::msg)?;
    }
    Ok(())
}

// read-mcp-limit
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn joppa_objects_takes_a_limit_up_to_100_with_fields() -> Result<()> {
    let w = record_world::world(&[]).await?;
    fixture(&w).await?;
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    let url = format!("{}/mcp", w.root);
    let reply: Value = w
        .http
        .post(url)
        .bearer_auth(&w.bot)
        .json(&list)
        .send()
        .await?
        .json()
        .await?;
    let tools = reply["result"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let limit = |name: &str| {
        let tool = tools.iter().find(|t| t["name"] == name);
        tool.map_or(Value::Null, |t| {
            t["inputSchema"]["properties"]["limit"].clone()
        })
    };
    let objects = limit("joppa_objects");
    let text = objects["description"].as_str().unwrap_or_default();
    ensure!(
        objects["minimum"] == 1
            && objects["maximum"] == 100
            && text.contains("25")
            && text.contains("100")
            && text.contains("fields"),
        "joppa_objects: {objects}"
    );
    // joppa_timeline is removed, so tools/list may not name it.
    for name in ["joppa_search", "joppa_timeline", "joppa_history"] {
        let limit = limit(name);
        ensure!(
            (name == "joppa_timeline" && limit.is_null())
                || (limit["minimum"] == 1 && limit["maximum"] == 25),
            "{name}: {limit}"
        );
    }
    // Without fields a page has 25 items. With fields it has 100, also for limit 101.
    let plain = read(&w, json!({"parent":"requirement:r","limit":100})).await?;
    let named = json!({"parent":"requirement:r","limit":100,"fields":["title"]});
    let named = read(&w, named).await?;
    let over = json!({"parent":"requirement:r","limit":101,"fields":["title"]});
    let over = read(&w, over).await?;
    ensure!(
        count(&plain) == 25
            && count(&named) == 100
            && named["next_cursor"].is_string()
            && count(&over) == 100,
        "{plain} {named} {over}"
    );
    for page in [&plain, &named, &over] {
        ensure!(page["total"] == 120, "{page}");
    }
    // relations_of and in_relation (also with fields) keep pages of 25.
    let edges = json!({"relations_of":"requirement:r","direction":"incoming","limit":100});
    let edges = read(&w, edges).await?;
    let members = json!({"in_relation":"related","limit":100,"fields":["title"]});
    let members = read(&w, members).await?;
    ensure!(
        count(&edges) == 25 && count(&members) == 25,
        "{edges} {members}"
    );
    Ok(())
}
