//! requirement:owner-manages-workspace over a real server: each member of a workspace hides it
//! only for that member. Each human member changes the title, an agent cannot, and a removed
//! member leaves no flag.
#[path = "common/hidden_lists.rs"]
mod hidden_lists;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use hidden_lists::{hide, items, listed, ok, refused, web};
use serde_json::{Value, json};

// ac:sfon2kd8
// test:3ya57e9z
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_human_member_changes_the_title_and_each_member_hides_for_the_own_list() -> Result<()>
{
    // bob (actor owner) created W. alex is a human member who did not create W.
    let w = record_world::world(&[]).await?;
    ok(
        &w,
        &w.alex,
        json!({"op":"update_workspace","hidden":true,"title":"X"}),
    )
    .await?;
    ok(&w, &w.alex, json!({"op":"update_workspace","title":"Y"})).await?;
    for token in [&w.alex, &w.owner] {
        let item = listed(&w, token).await?;
        ensure!(item["title"] == "Y", "the title of alex: {item}");
    }
    let text = refused(&w, &w.alex, json!({"op":"update_workspace"})).await?;
    ensure!(
        text.contains("update_workspace needs title, workspace_id, reference_prefix or hidden"),
        "{text}"
    );
    ok(&w, &w.alex, hide(true)).await?;
    let text = refused(&w, &w.bot, json!({"op":"update_workspace","title":"Z"})).await?;
    ensure!(
        text.contains("a human member updates the workspace"),
        "{text}"
    );
    let old = json!({"op":"rename_workspace","title":"Old","reason":"Rename"});
    let text = refused(&w, &w.owner, old).await?;
    ensure!(
        text.contains("rename_workspace was removed; use update_workspace"),
        "{text}"
    );
    // eve has no membership and no grant of the workspace.
    let text = refused(&w, &w.outsider, hide(true)).await?;
    ensure!(text.contains("Workspace not found"), "{text}");
    let item = listed(&w, &w.alex).await?;
    ensure!(
        item["hidden"] == true && item["title"] == "Y",
        "joppa_list of alex: {item}"
    );
    let item = listed(&w, &w.owner).await?;
    ensure!(
        item.get("hidden").is_none() && item["title"] == "Y",
        "joppa_list of bob: {item}"
    );
    Ok(())
}

/// joppa_list of alex gives W without hidden, and GET /api/workspaces of alex lists W.
async fn alex_sees_it(w: &record_world::World, moment: &str) -> Result<()> {
    let item = listed(w, &w.alex).await?;
    ensure!(
        item.get("hidden").is_none(),
        "{moment}: joppa_list of alex: {item}"
    );
    let lists = web(w, &w.alex, "/api/workspaces").await?;
    ensure!(
        lists.contains(&w.ws),
        "{moment}: GET /api/workspaces of alex"
    );
    Ok(())
}

// ac:nikye1a1
// test:d406qlpg
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_removed_member_comes_back_without_the_hidden_flag() -> Result<()> {
    let mut w = record_world::world(&[]).await?;
    ok(&w, &w.alex, hide(true)).await?;
    ensure!(listed(&w, &w.alex).await?["hidden"] == true, "alex hides");
    let remove = json!({"op":"remove_member","id":"alex","reason":"Left the team"});
    ok(&w, &w.owner, remove).await?;
    let add = json!({"op":"member","id":"alex","role":"human"});
    ok(&w, &w.owner, add).await?;
    alex_sees_it(&w, "after the new membership").await?;
    hidden_lists::rebuild(&mut w).await?;
    alex_sees_it(&w, "after the rebuild").await?;
    Ok(())
}

// ac:6asqz4s1
// test:dxn3y6ru
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_full_web_list_gives_the_hidden_workspaces_of_the_caller() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let w2 = hidden_lists::second_workspace(&w).await?;
    ok(&w, &w.alex, hide(true)).await?;
    // Other tests of this target add alex to other workspaces: read only W and W2.
    let ours = |list: Vec<Value>| -> Vec<Value> {
        list.into_iter()
            .filter(|i| i["id"] == w.ws.as_str() || i["id"] == w2.as_str())
            .collect()
    };
    let all = ours(items(&w, &w.alex, "/api/workspaces?all=true").await?);
    let of = |id: &str| all.iter().find(|i| i["id"] == id).cloned();
    let first = of(&w.ws).unwrap_or(Value::Null);
    ensure!(first["hidden"] == true, "?all=true: W {all:?}");
    let second = of(&w2).unwrap_or(Value::Null);
    ensure!(
        second.is_object() && second.get("hidden").is_none(),
        "?all=true: W2 {all:?}"
    );
    ensure!(all.len() == 2, "?all=true: {all:?}");
    let plain = ours(items(&w, &w.alex, "/api/workspaces").await?);
    let ids: Vec<&Value> = plain.iter().map(|i| &i["id"]).collect();
    ensure!(ids == [&json!(w2)], "GET /api/workspaces: {plain:?}");
    Ok(())
}
