//! requirement:task-free-edits over a real server: an agent alias is an assignee that the list
//! items, the assignee filter and the board read show.
#[path = "common/generic_world.rs"]
mod generic_world;
use anyhow::{Result, ensure};
use generic_world::{listed, save, world};
use serde_json::json;

// ac:8u8nl8pt
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_alias_is_an_assignee_that_lists_and_filters_show() -> Result<()> {
    let w = world(&[]).await?;
    let data = |title: &str| json!({"title":title,"owner":"owner","body":"Do the work"});
    let mut held = data("Held");
    held["assignee"] = json!("lane3");
    save(&w, "task", "t1", "ac:a", held, json!({})).await?;
    save(&w, "task", "t2", "ac:a", data("Free"), json!({})).await?;
    let page = w
        .mcp(&w.bot, "joppa_objects", json!({"kind":"task","parent":"*"}))
        .await?;
    let page = page.map_err(anyhow::Error::msg)?;
    let item = |address: &str| {
        let items = page["items"].as_array().into_iter().flatten();
        items
            .into_iter()
            .find(|i| i["address"] == address)
            .cloned()
            .unwrap_or_default()
    };
    ensure!(item("task:t1")["assignee"] == "lane3", "{page}");
    ensure!(item("task:t2").get("assignee").is_none(), "{page}");
    let filtered = listed(&w, json!({"kind":"task","assignee":"lane3","parent":"*"})).await?;
    ensure!(filtered == ["task:t1"], "{filtered:?}");
    let url = format!("{}/api/workspaces/{}/board", w.root, w.ws);
    let board: serde_json::Value = w
        .http
        .get(url)
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    ensure!(
        board["assignees"] == json!(["lane3"]),
        "{}",
        board["assignees"]
    );
    Ok(())
}
