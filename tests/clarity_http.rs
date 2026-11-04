//! MCP replies tell an agent what it needs next, against a real server.
//! Each test names the point of note clarity-gaps-20261021 that it proves.
#[path = "common/record_world.rs"]
mod record_world;

use anyhow::{Result, ensure};
use record_world::{A, world};
use serde_json::{Value, json};

const TEST: &str = "tests/a.rs > a";

fn ok(reply: Result<Value, String>, what: &str) -> Result<Value> {
    reply.map_err(|e| anyhow::anyhow!("{what}: {e}"))
}

// clarity point 1
#[tokio::test]
async fn a_requirement_read_marks_an_archived_requirement_and_an_archived_criterion() -> Result<()>
{
    let w = world(&[TEST]).await?;
    for command in [
        json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":"requirement:r","data":{"title":"B","owner":"owner"},"reason":"Create"}),
        json!({"op":"archive_object","target":"ac:b","revision":1,"reason":"B is out of scope"}),
        json!({"op":"save_object","kind":"requirement","id":"old","revision":0,"parent":null,"data":{"title":"Old","owner":"owner","body":"Old rule"},"reason":"Create"}),
        json!({"op":"archive_object","target":"requirement:old","revision":1,"reason":"Old is replaced"}),
    ] {
        ok(w.command(&w.owner, command).await?, "setup")?;
    }
    let read = |address: &'static str| {
        let w = &w;
        async move {
            ok(
                w.mcp(&w.bot, "joppa_objects", json!({"address":address}))
                    .await?,
                address,
            )
        }
    };
    let live = read("requirement:r").await?;
    // A present object has no present key; an archived one gives present: false.
    ensure!(live.get("present").is_none(), "{live}");
    let children = ok(
        // Depth 1 gives the direct children of R, without the test node of A.
        w.mcp(
            &w.bot,
            "joppa_objects",
            json!({"parent":"requirement:r","depth":1}),
        )
        .await?,
        "children of r",
    )?;
    let listed: Vec<&str> = children["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["address"].as_str())
        .collect();
    ensure!(listed == ["ac:a"], "{children}");
    let archived = read("ac:b").await?;
    ensure!(archived["present"] == false, "{archived}");
    let old = read("requirement:old").await?;
    ensure!(old["present"] == false, "{old}");
    Ok(())
}

// clarity point 3
#[tokio::test]
async fn help_with_a_workspace_gives_one_save_example_for_each_type() -> Result<()> {
    let w = world(&[TEST]).await?;
    let help = ok(
        w.mcp(&w.bot, "joppa_help", json!({"section":"examples"}))
            .await?,
        "help",
    )?;
    let examples = &help["examples"];
    ensure!(
        examples["ac"]["op"] == "save_object" && examples["ac"]["kind"] == "ac",
        "{examples}"
    );
    ensure!(examples["ac"]["parent"] == "<requirement:id>", "{examples}");
    ensure!(examples["ac"]["data"]["body"].is_string(), "{examples}");
    ensure!(
        examples["requirement"]["parent"].is_null(),
        "a root type has no parent: {examples}"
    );
    ensure!(
        examples["requirement"]["data"]["body"].is_string(),
        "{examples}"
    );
    Ok(())
}

// clarity point 6
#[tokio::test]
async fn a_criterion_save_keeps_its_test_nodes() -> Result<()> {
    let w = world(&[TEST]).await?;
    // The tests of A are its test nodes: a text change keeps them, and no draft waits for
    // another save. A save with a test list in the document is refused.
    let listed = json!({"op":"save_object","kind":"ac","id":"a","revision":1,"parent":"requirement:r",
        "data":{"title":"A","owner":"owner","body":"Given R, then A holds.","tests":[TEST]},"reason":"Clarify A"});
    let refused = w.command(&w.bot, listed).await?;
    ensure!(
        refused
            .as_ref()
            .err()
            .is_some_and(|e| e.contains("use save_object with kind test")),
        "{refused:?}"
    );
    let saved = ok(
        w.command(
            &w.bot,
            json!({"op":"save_object","kind":"ac","id":"a","revision":1,"parent":"requirement:r",
        "data":{"title":"A","owner":"owner","body":"Given R, then A holds."},"reason":"Clarify A"}),
        )
        .await?,
        "save",
    )?;
    ensure!(saved["object"].get("tests_draft").is_none(), "{saved}");
    let nodes = json!({"parent": A, "kind": "test"});
    let nodes = ok(w.mcp(&w.alex, "joppa_objects", nodes).await?, "test nodes")?;
    ensure!(
        nodes["total"] == 1 && nodes["items"][0]["title"] == TEST,
        "{nodes}"
    );
    let status = w.a_status(&w.alex).await?;
    ensure!(
        status["tests"].as_array().map(Vec::len) == Some(1)
            && status["tests"][0]["address"] == TEST,
        "{status}"
    );
    Ok(())
}

// clarity point 8
#[tokio::test]
async fn fields_make_large_list_replies_small() -> Result<()> {
    let w = world(&[TEST]).await?;
    let list = ok(
        w.mcp(
            &w.bot,
            "joppa_objects",
            json!({"parent":"*","fields":["title"]}),
        )
        .await?,
        "objects",
    )?;
    let items = list["items"].as_array().cloned().unwrap_or_default();
    ensure!(!items.is_empty(), "{list}");
    for item in &items {
        let keys: Vec<&String> = item
            .as_object()
            .map(|o| o.keys().collect())
            .unwrap_or_default();
        ensure!(
            keys.iter().all(|k| *k == "address" || *k == "title"),
            "{item}"
        );
    }
    Ok(())
}

// agent-clarity-ac10
#[tokio::test]
async fn the_write_schema_names_data_owner_and_a_save_without_it_is_refused() -> Result<()> {
    let w = world(&[TEST]).await?;
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
    for name in ["joppa_command", "joppa_batch"] {
        let tool = reply["result"]["tools"]
            .as_array()
            .and_then(|t| t.iter().find(|t| t["name"] == name));
        let text = tool.map(Value::to_string).unwrap_or_default();
        ensure!(
            text.contains("data.owner: the member ID of a human owner, required on every save"),
            "{name} does not name data.owner"
        );
    }
    let save = json!({"op":"save_object","kind":"requirement","id":"nobody","revision":0,
        "parent":null,"data":{"title":"No owner","body":"A rule"},"reason":"Create"});
    let refused = w.command(&w.bot, save).await?;
    ensure!(
        refused == Err("missing text field owner".into()),
        "{refused:?}"
    );
    Ok(())
}
