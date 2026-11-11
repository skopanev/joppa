//! requirement:owner-manages-workspace over a real server: update_workspace hides a workspace
//! from the web lists of the member who sends it, and every MCP read and write of it works as
//! before.
#[path = "common/hidden_lists.rs"]
mod hidden_lists;
#[path = "common/http_command.rs"]
mod http_command;
#[path = "common/legacy_event.rs"]
mod legacy_event;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use hidden_lists::{get, hide, listed, ok, web};
use http_command::ok as admin;
use record_world::World;
use serde_json::json;

/// The lists of O and alex after one step: the web list of each, the joppa_list item of each
/// with hidden: true only for a member in `hiders`, and GET /api/access of O with W.
async fn step(w: &World, hiders: &[&str], step: &str) -> Result<()> {
    for (token, name) in [(&w.owner, "owner"), (&w.alex, "alex")] {
        let hides = hiders.contains(&name);
        let lists = web(w, token, "/api/workspaces").await?.contains(&w.ws);
        ensure!(lists != hides, "{step}: GET /api/workspaces of {name}");
        let item = listed(w, token).await?;
        let flag = item.get("hidden");
        let right = if hides {
            flag == Some(&json!(true))
        } else {
            flag.is_none()
        };
        ensure!(right, "{step}: joppa_list of {name}: {item}");
    }
    let access = web(w, &w.owner, "/api/access").await?;
    ensure!(
        access.contains(&w.ws),
        "{step}: GET /api/access of the owner"
    );
    Ok(())
}

// ac:mwr6am3k
// MCP hides update_workspace: each update_workspace goes through POST /api/workspaces/W/commands.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_hidden_workspace_leaves_the_web_lists_and_keeps_its_mcp_reads_and_writes() -> Result<()>
{
    let w = record_world::world(&[]).await?;
    let ws = w.ws.clone();
    step(&w, &[], "before").await?;
    // The member alex hides the workspace only for alex.
    admin(&w, &w.alex, hide(true)).await?;
    step(&w, &["alex"], "alex hides").await?;
    // The owner hides the workspace without a reason.
    admin(&w, &w.owner, hide(true)).await?;
    step(&w, &["owner", "alex"], "the owner hides").await?;
    for token in [&w.owner, &w.alex] {
        let (code, board) = get(&w, token, &format!("/api/workspaces/{ws}/board")).await?;
        ensure!(
            code == 200 && board["workspace"] == ws.as_str(),
            "{code} {board}"
        );
    }
    let save = json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":"requirement:r",
        "data":{"title":"B","owner":"owner"},"reason":"Create"});
    ok(&w, &w.bot, save).await?;
    let b = w
        .mcp(&w.bot, "joppa_objects", json!({"address":"ac:b"}))
        .await?;
    ensure!(
        b.map_err(anyhow::Error::msg)?["data"]["title"] == "B",
        "joppa_objects of ac:b"
    );
    // A new title keeps the workspace hidden. hidden: false shows it again.
    let title = json!({"op":"update_workspace","title":"Hidden channel"});
    admin(&w, &w.owner, title).await?;
    step(&w, &["owner", "alex"], "a new title").await?;
    ensure!(
        listed(&w, &w.owner).await?["title"] == "Hidden channel",
        "the new title"
    );
    let show = json!({"op":"update_workspace","hidden":false,"reason":"Show again"});
    admin(&w, &w.owner, show).await?;
    step(&w, &["alex"], "the owner shows").await?;
    admin(&w, &w.alex, hide(false)).await?;
    step(&w, &[], "alex shows").await?;
    let item = listed(&w, &w.alex).await?;
    ensure!(item["title"] == "Hidden channel", "{item}");
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
        web(&w, &w.owner, "/api/workspaces").await?.contains(&w.ws),
        "the web list"
    );
    Ok(())
}
