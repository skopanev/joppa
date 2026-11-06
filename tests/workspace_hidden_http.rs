//! requirement:owner-manages-workspace over a real server: update_workspace hides a workspace
//! from the web lists, and every MCP read and write of it works as before.
#[path = "common/legacy_event.rs"]
mod legacy_event;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use record_world::World;
use serde_json::{Value, json};

/// A GET of the server with the owner token: the HTTP code and the body.
async fn get(w: &World, path: &str) -> Result<(u16, Value)> {
    let url = format!("{}{path}", w.root);
    let reply = w.http.get(url).bearer_auth(&w.owner).send().await?;
    Ok((reply.status().as_u16(), reply.json().await?))
}

/// The workspace IDs of a web list: GET /api/workspaces, or the items of GET /api/access.
async fn web(w: &World, path: &str) -> Result<Vec<String>> {
    let (code, body) = get(w, path).await?;
    ensure!(code == 200, "{path}: {code} {body}");
    let items = body.get("items").unwrap_or(&body).as_array().cloned();
    let items = items.with_context(|| format!("{path}: {body}"))?;
    Ok(items
        .iter()
        .filter_map(|i| i["id"].as_str().map(String::from))
        .collect())
}

/// The joppa_list item of the world workspace for the member of `token`.
async fn listed(w: &World, token: &str) -> Result<Value> {
    let list = w.mcp(token, "joppa_list", json!({})).await?;
    let list = list.map_err(anyhow::Error::msg)?;
    let items = list["items"].as_array().into_iter().flatten();
    items
        .into_iter()
        .find(|i| i["id"] == w.ws.as_str())
        .cloned()
        .context("joppa_list item")
}

/// The refusal of one command, or an error when the command succeeds.
async fn refused(w: &World, token: &str, command: Value) -> Result<String> {
    match w.command(token, command.clone()).await? {
        Err(text) => Ok(text),
        Ok(reply) => anyhow::bail!("{command} was accepted: {reply}"),
    }
}

// ac:mwr6am3k
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_hidden_workspace_leaves_the_web_lists_and_keeps_its_mcp_reads_and_writes() -> Result<()>
{
    let w = record_world::world(&[]).await?;
    let ws = w.ws.clone();
    for path in ["/api/workspaces", "/api/access"] {
        ensure!(web(&w, path).await?.contains(&ws), "{path} before");
    }
    let hide = json!({"op":"update_workspace","hidden":true});
    let text = refused(&w, &w.alex, hide.clone()).await?;
    ensure!(
        text.contains("workspace owner updates the workspace"),
        "{text}"
    );
    let text = refused(
        &w,
        &w.owner,
        json!({"op":"update_workspace","reason":"Nothing"}),
    )
    .await?;
    ensure!(
        text.contains("update_workspace needs title, workspace_id, reference_prefix or hidden"),
        "{text}"
    );
    let old = json!({"op":"rename_workspace","title":"Old","reason":"Rename"});
    let text = refused(&w, &w.owner, old).await?;
    ensure!(
        text.contains("rename_workspace was removed; use update_workspace"),
        "{text}"
    );
    // The owner hides the workspace without a reason.
    w.command(&w.owner, hide)
        .await?
        .map_err(anyhow::Error::msg)?;
    for path in ["/api/workspaces", "/api/access"] {
        ensure!(
            !web(&w, path).await?.contains(&ws),
            "{path} lists the hidden workspace"
        );
    }
    for token in [&w.owner, &w.alex] {
        let item = listed(&w, token).await?;
        ensure!(
            item["hidden"] == true && item["title"] == "Record channel",
            "{item}"
        );
    }
    let (code, board) = get(&w, &format!("/api/workspaces/{ws}/board")).await?;
    ensure!(
        code == 200 && board["workspace"] == ws.as_str(),
        "{code} {board}"
    );
    let save = json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":"requirement:r",
        "data":{"title":"B","owner":"owner"},"reason":"Create"});
    w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    let b = w
        .mcp(&w.bot, "joppa_objects", json!({"address":"ac:b"}))
        .await?;
    ensure!(
        b.map_err(anyhow::Error::msg)?["data"]["title"] == "B",
        "joppa_objects of ac:b"
    );
    // A new title keeps the workspace hidden. hidden: false shows it again.
    let title = json!({"op":"update_workspace","title":"Hidden channel"});
    w.command(&w.owner, title)
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(
        listed(&w, &w.owner).await?["hidden"] == true,
        "a new title shows the workspace"
    );
    let show = json!({"op":"update_workspace","hidden":false,"reason":"Show again"});
    w.command(&w.owner, show)
        .await?
        .map_err(anyhow::Error::msg)?;
    for path in ["/api/workspaces", "/api/access"] {
        ensure!(web(&w, path).await?.contains(&ws), "{path} after");
    }
    let item = listed(&w, &w.owner).await?;
    ensure!(
        item.get("hidden").is_none() && item["title"] == "Hidden channel",
        "{item}"
    );
    Ok(())
}

// ac:mwr6am3k
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_old_journal_rename_still_names_the_workspace_after_a_rebuild() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let old = json!({"op":"rename_workspace","title":"Old journal name","reason":"Rename"});
    legacy_event::execute(&w.pool, &w.ws, "owner", old).await?;
    let item = listed(&w, &w.owner).await?;
    ensure!(item["title"] == "Old journal name", "{item}");
    ensure!(
        web(&w, "/api/workspaces").await?.contains(&w.ws),
        "the web list"
    );
    Ok(())
}
