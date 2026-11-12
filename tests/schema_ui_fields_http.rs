//! requirement:tree-schema-safe over a real server: set_tree_schema keeps the UI fields of a
//! type that the command omits, so a type copied from the MCP tree keeps its board colours.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use record_world::World;
use serde_json::{Value, json};

/// One GET of the owner below /api/workspaces/<W>.
async fn get(w: &World, path: &str) -> Result<Value> {
    let url = format!("{}/api/workspaces/{}{path}", w.root, w.ws);
    let reply = w.http.get(url).bearer_auth(&w.owner).send().await?;
    let code = reply.status();
    let body: Value = reply.json().await?;
    ensure!(code.is_success(), "GET {path}: {code} {body}");
    Ok(body)
}

/// The MCP tree of the bot.
async fn tree(w: &World) -> Result<Value> {
    w.mcp(&w.bot, "joppa_tree", json!({}))
        .await?
        .map_err(anyhow::Error::msg)
}

/// One set_tree_schema of `token` at the current revision with the keys of `change`. Gives the
/// new schema revision.
async fn schema(w: &World, token: &str, change: Value) -> Result<u64> {
    let revision = tree(w).await?["schema"]["revision"].as_u64().unwrap_or(0);
    let mut command = json!({"op":"set_tree_schema","revision":revision,"reason":"UI fields"});
    for (k, v) in change.as_object().into_iter().flatten() {
        command[k] = v.clone();
    }
    w.command(token, command)
        .await?
        .map_err(anyhow::Error::msg)?;
    Ok(revision + 1)
}

/// The UI fields color, badge, index and title_size of `kind` in the HTTP tree at `revision`.
async fn ui(w: &World, revision: u64, kind: &str) -> Result<Value> {
    let read = get(w, &format!("/tree?revision={revision}")).await?;
    let t = &read["schema"]["types"][kind];
    ensure!(
        t.is_object(),
        "revision {revision} has no type {kind}: {read}"
    );
    Ok(json!([t["color"], t["badge"], t["index"], t["title_size"]]))
}

// ac:iolhwin0
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_type_copied_from_the_mcp_tree_keeps_its_ui_fields() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let ac = json!({"title":"Criteria","role":"ac","status":"children","parents":["requirement"],
        "color":"#63756b","badge":"AC","index":3,"title_size":14});
    let n = schema(&w, &w.owner, json!({"put_types":{"ac":ac}})).await?;
    ensure!(
        ui(&w, n, "ac").await? == json!(["#63756b", "AC", 3, 14]),
        "N"
    );
    // The bot copies ac from joppa_tree and changes its question.
    let mut copy = tree(&w).await?["schema"]["types"]["ac"].clone();
    copy["question"] = json!("What must hold?");
    let first = schema(&w, &w.bot, json!({"put_types":{"ac":copy}})).await?;
    ensure!(first == n + 1, "N+1 is {first}");
    let read = get(&w, &format!("/tree?revision={first}")).await?;
    ensure!(
        read["schema"]["types"]["ac"]["question"] == "What must hold?",
        "N+1: {read}"
    );
    ensure!(
        ui(&w, first, "ac").await? == json!(["#63756b", "AC", 3, 14]),
        "N+1 keeps the UI fields"
    );
    // A command that names color and badge changes only them.
    let mut copy = tree(&w).await?["schema"]["types"]["ac"].clone();
    copy["color"] = json!("#112233");
    copy["badge"] = json!("CRIT");
    let second = schema(&w, &w.bot, json!({"put_types":{"ac":copy}})).await?;
    let changed = json!(["#112233", "CRIT", 3, 14]);
    ensure!(ui(&w, second, "ac").await? == changed, "N+2");
    // The whole lean map of joppa_tree as types keeps the four fields of ac.
    let types = tree(&w).await?["schema"]["types"].clone();
    let third = schema(&w, &w.bot, json!({"types":types})).await?;
    ensure!(ui(&w, third, "ac").await? == changed, "N+3");
    // A new type without UI fields gets none.
    let risk = json!({"title":"Risks","role":"document","status":"none","root":true,"parents":[]});
    let fourth = schema(&w, &w.bot, json!({"put_types":{"risk":risk}})).await?;
    let none = json!([null, null, null, null]);
    ensure!(ui(&w, fourth, "risk").await? == none, "risk");
    ensure!(ui(&w, fourth, "ac").await? == changed, "ac after risk");
    // The board shows ac with the badge CRIT and the colour #112233.
    let board = get(&w, "/board").await?;
    let shown = &board["object_types"]["ac"];
    ensure!(
        shown["badge"] == "CRIT" && shown["color"] == "#112233",
        "board: {shown}"
    );
    Ok(())
}
