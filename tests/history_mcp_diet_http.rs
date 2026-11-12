//! requirement ea0ca1c0 over a real server: an MCP History entry gives no kind, no signal, no
//! change equal to its op, and an at with whole seconds. HTTP History keeps them.
#[path = "common/mcp_shape.rs"]
mod mcp_shape;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use record_world::World;
use serde_json::{Value, json};

async fn mcp(w: &World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args).await?.map_err(anyhow::Error::msg)
}

/// The entry with op `op` of a page.
fn entry<'a>(page: &'a Value, op: &str) -> Result<&'a Value> {
    let items = page["items"].as_array().context("items")?;
    items
        .iter()
        .find(|i| i["op"] == op)
        .with_context(|| format!("no {op} entry: {page}"))
}

// ac:jkvdy2jl
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_mcp_entry_gives_no_kind_no_repeated_change_and_whole_seconds() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let p = w.position().await?.as_u64().context("position")?;
    let save = json!({"op":"save_object","kind":"requirement","id":"q","revision":0,
        "parent":null,"data":{"title":"Q","owner":"owner","body":"A rule"},"reason":"Create"});
    let saved = mcp(
        &w,
        "joppa_command",
        json!({"request_id":"diet-save","command":save}),
    )
    .await?;
    let position = saved["position"].as_u64().context("save position")?;
    let note = json!({"op":"note","target":"requirement:q","id":"c1","text":"A question"});
    w.command(&w.bot, note).await?.map_err(anyhow::Error::msg)?;
    let mark = json!({"position":position,"reason":"Read"});
    mcp(&w, "joppa_history_mark", mark).await?;
    let page = mcp(&w, "joppa_history", json!({"after":p})).await?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    ensure!(items.len() == 2, "{page}");
    for item in &items {
        ensure!(
            item.get("kind").is_none()
                && item.get("signal").is_none()
                && item.get("change") != item.get("op")
                && (item["status"] == "open" || item["status"] == "processed"),
            "{item}"
        );
    }
    mcp_shape::whole_seconds(&page).map_err(anyhow::Error::msg)?;
    let saved = entry(&page, "save_object")?;
    ensure!(
        saved["change"] == "save" && saved["status"] == "processed",
        "{saved}"
    );
    let comment = entry(&page, "note")?;
    ensure!(
        comment["change"] == "comment" && comment["comment_id"] == "c1",
        "{comment}"
    );
    // With open true each entry is open: no entry gives its status.
    let open = mcp(&w, "joppa_history", json!({"after":p,"open":true})).await?;
    let items = open["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        !items.is_empty() && items.iter().all(|i| i.get("status").is_none()),
        "{open}"
    );
    // HTTP keeps kind, status open and the fraction of a second.
    let url = format!("{}/api/workspaces/{}/history?after={p}", w.root, w.ws);
    let reply = w.http.get(url).bearer_auth(&w.alex).send().await?;
    let http: Value = reply.error_for_status()?.json().await?;
    let note = entry(&http, "note")?;
    let fraction = http["items"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|i| i["at"].as_str().is_some_and(|at| at.contains('.')));
    ensure!(
        entry(&http, "save_object")?["kind"] == "requirement"
            && note["status"] == "open"
            && fraction,
        "HTTP: {http}"
    );
    mcp_shape::same_history(&http, &page).map_err(anyhow::Error::msg)?;
    Ok(())
}
