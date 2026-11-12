//! requirement:read-contract over a real server: an MCP write receipt echoes no text, links,
//! reason, question, commit or actor. POST /api/workspaces/{w}/commands keeps the full receipt.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use joppa::{commands::Command, store};
use record_world::World;
use serde_json::{Value, json};

const T: &str = "tests/x.rs > t";
const QUESTION: &str = "Which observable result proves the rule?";

fn absent(value: &Value, keys: &[&str]) -> bool {
    keys.iter().all(|k| value.get(*k).is_none())
}

/// One MCP command of `token` that must succeed.
async fn mcp(w: &World, token: &str, command: Value) -> Result<Value> {
    w.command(token, command).await?.map_err(anyhow::Error::msg)
}

/// One command of `token` through POST /api/workspaces/{w}/commands that must succeed.
async fn http(w: &World, token: &str, command: Value) -> Result<Value> {
    let url = format!("{}/api/workspaces/{}/commands", w.root, w.ws);
    let body = json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command});
    let reply = w
        .http
        .post(url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;
    let code = reply.status();
    let body: Value = reply.json().await?;
    ensure!(code.is_success(), "HTTP: {code} {body}");
    Ok(body)
}

/// Workspace V with criterion ac:x at revision 1. Owner creates it, and bot is its agent.
async fn target_workspace(w: &World) -> Result<String> {
    let v = format!("receipt-{}", uuid::Uuid::new_v4().simple());
    let create: Command = serde_json::from_value(json!({"op":"workspace","title":"Target"}))?;
    store::execute(&w.pool, &v, "owner", "setup-v", &create).await?;
    store::enroll_service(&w.pool, &v, "owner", "bot", "agent").await?;
    let tree = store::tree_schema(&w.pool, &v, "owner", None).await?;
    for (actor, command) in [
        (
            "owner",
            json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
            "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
            "ac":{"title":"Criteria","role":"ac","status":"children","parents":["requirement"]}}}),
        ),
        (
            "bot",
            json!({"op":"save_object","kind":"requirement","id":"r","revision":0,"parent":null,
            "data":{"title":"R","owner":"owner","body":"The API rule"},"reason":"Create"}),
        ),
        (
            "bot",
            json!({"op":"save_object","kind":"ac","id":"x","revision":0,"parent":"requirement:r",
            "data":{"title":"X","owner":"owner"},"reason":"Create"}),
        ),
    ] {
        let id = uuid::Uuid::new_v4().to_string();
        store::execute_remote_input(&w.pool, &v, actor, &id, &command).await?;
    }
    Ok(v)
}

/// A save of ac:a with a new body, at its current revision.
async fn save_a(w: &World, body: &str) -> Result<(Value, Value)> {
    let revision = w.object("ac:a").await?["revision"].clone();
    let save = json!({"op":"save_object","kind":"ac","id":"a","revision":revision,
        "parent":"requirement:r","data":{"body":body},"reason":"Clarify the criterion"});
    Ok((save, revision))
}

// ac:flg87alq
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_mcp_write_receipt_echoes_no_text_links_reason_or_question() -> Result<()> {
    let w = record_world::world(&[T]).await?;
    let v = target_workspace(&w).await?;
    // The schema write gives ac a question. Its command gives no reason.
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let revision = tree.map_err(anyhow::Error::msg)?["schema"]["revision"].clone();
    let ac = json!({"title":"Criteria","role":"ac","status":"children","parents":["requirement"],
        "question":QUESTION});
    let schema =
        json!({"op":"set_tree_schema","revision":revision,"put_types":{"ac":ac},"reason":"Add"});
    let schema = mcp(&w, &w.owner, schema).await?;
    ensure!(
        schema["command"] == json!({"op":"set_tree_schema","revision":revision})
            && absent(&schema, &["summary", "system_ids"]),
        "schema: {schema}"
    );
    // The note command names the note and not its text.
    let note = json!({"op":"note","target":"ac:a","id":"c1","text":"Long finding"});
    let noted = mcp(&w, &w.bot, note).await?;
    ensure!(
        noted["command"] == json!({"op":"note","target":"ac:a","id":"c1"}),
        "note: {noted}"
    );
    // The remote links command gives no links.
    let link = json!({"workspace":v,"target":"ac:x","relation":"related","revision":1});
    let links = json!({"op":"set_remote_links","target":"ac:a","revision":0,"links":[link],
        "reason":"Uses X"});
    let linked = mcp(&w, &w.bot, links).await?;
    ensure!(
        linked["command"]["op"] == "set_remote_links"
            && absent(&linked["command"], &["links"])
            && absent(&linked, &["summary", "system_ids"]),
        "set_remote_links: {linked}"
    );
    // The save gives the address and the revision of the object, and not the question.
    let (save, revision) = save_a(&w, "Agent body").await?;
    let saved = mcp(&w, &w.bot, save).await?;
    ensure!(
        saved["object"]["address"] == "ac:a"
            && saved["object"]["revision"].as_u64() == revision.as_u64().map(|r| r + 1)
            && absent(&saved["object"], &["question"])
            && absent(&saved, &["summary", "system_ids"]),
        "save: {saved}"
    );
    // The mark of the open save entry gives no reason.
    let position = saved["position"].clone();
    ensure!(position.is_u64(), "save: {saved}");
    let marked = w
        .mcp(
            &w.bot,
            "joppa_history_mark",
            json!({"position":position,"reason":"Read"}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(absent(&marked, &["reason"]), "mark: {marked}");
    // The record of one bound test gives no commit and no actor.
    let record = w
        .recorded(&"a".repeat(40), json!([{"test":T,"status":"pass"}]))
        .await?;
    ensure!(
        absent(&record, &["commit", "actor"]) && record["unbound"] == json!([]),
        "record: {record}"
    );
    // A member sends the same note and save over HTTP: the replies keep the text and question.
    let note = json!({"op":"note","target":"ac:a","id":"c2","text":"Long finding"});
    let noted = http(&w, &w.alex, note).await?;
    ensure!(
        noted["command"]["text"] == "Long finding",
        "HTTP note: {noted}"
    );
    let (save, _) = save_a(&w, "Member body").await?;
    let saved = http(&w, &w.alex, save).await?;
    ensure!(
        saved["object"]["question"] == QUESTION,
        "HTTP save: {saved}"
    );
    Ok(())
}
