//! The assignee filter over a real server: an empty assignee gives the nodes without an
//! assignee, in joppa_objects, joppa_search and GET /objects. requirement:r holds task:t1 of
//! alex, task:t2 without an assignee and task:t3 with the empty assignee "".
#[path = "common/plain_http.rs"]
mod plain_http;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use plain_http::{property, sorted, tools};
use record_world::World;
use relations_world::{msg, saved};
use serde_json::{Value, json};

/// The tasks that the assignee filter `assignee` keeps, read three ways. All three must give
/// the same addresses.
async fn kept(w: &World, assignee: &str) -> Result<Vec<String>> {
    let filter = json!({"kind":"task","assignee":assignee});
    let list = w.mcp(&w.bot, "joppa_objects", filter.clone()).await?;
    let list = sorted(&list.map_err(msg)?);
    let mut search = filter;
    search["query"] = json!("fixture");
    search["limit"] = json!(25);
    let found = w.mcp(&w.bot, "joppa_search", search).await?;
    let found = sorted(&found.map_err(msg)?);
    let url = format!("{}/api/workspaces/{}/objects", w.root, w.ws);
    let query = [("kind", "task"), ("assignee", assignee)];
    let reply = w.http.get(url).bearer_auth(&w.bot).query(&query);
    let reply = reply.send().await?;
    let code = reply.status();
    let page: Value = reply.json().await?;
    ensure!(
        code.is_success(),
        "GET /objects assignee {assignee:?}: {code} {page}"
    );
    let http = sorted(&page);
    ensure!(
        list == found && list == http,
        "assignee {assignee:?}: joppa_objects {list:?}, joppa_search {found:?}, GET {http:?}"
    );
    Ok(list)
}

// ac:544947gz
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_empty_assignee_filter_gives_the_nodes_without_an_assignee() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    let parent = "requirement:r";
    saved(&w, "task:t1", 0, json!({"parent":parent,"assignee":"alex"})).await?;
    saved(&w, "task:t2", 0, json!({"parent":parent})).await?;
    saved(&w, "task:t3", 0, json!({"parent":parent,"assignee":""})).await?;
    let empty = kept(&w, "").await?;
    ensure!(empty == ["task:t2", "task:t3"], "assignee \"\": {empty:?}");
    let alex = kept(&w, "alex").await?;
    ensure!(alex == ["task:t1"], "assignee alex: {alex:?}");
    let tools = tools(&w, &w.bot).await?;
    for name in ["joppa_objects", "joppa_search"] {
        let assignee = property(&tools, name, "assignee");
        let text = assignee["description"].as_str().unwrap_or_default();
        let names = text.contains("\"\"") && text.to_lowercase().contains("no assignee");
        ensure!(
            names,
            "{name}: assignee names \"\" as no assignee: {assignee}"
        );
    }
    Ok(())
}
