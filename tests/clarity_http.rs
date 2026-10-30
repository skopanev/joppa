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
        w.mcp(&w.bot, "joppa_objects", json!({"parent":"requirement:r"}))
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
    let help = ok(w.mcp(&w.bot, "joppa_help", json!({})).await?, "help")?;
    let examples = &help["examples"];
    ensure!(
        examples["ac"]["op"] == "save_object" && examples["ac"]["kind"] == "ac",
        "{examples}"
    );
    ensure!(examples["ac"]["parent"] == "<requirement:id>", "{examples}");
    ensure!(
        examples["ac"]["data"]["description"].is_string(),
        "{examples}"
    );
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
async fn a_criterion_save_keeps_its_test_list_in_the_document() -> Result<()> {
    let w = world(&[TEST]).await?;
    // The test list is the field tests of the document: a text change keeps it, and no
    // draft waits for another save.
    let saved = ok(w.command(&w.bot, json!({"op":"save_object","kind":"ac","id":"a","revision":2,"parent":"requirement:r",
        "data":{"title":"A","owner":"owner","description":"Given R, then A holds.","tests":[TEST]},"reason":"Clarify A"})).await?, "save")?;
    ensure!(saved["object"].get("tests_draft").is_none(), "{saved}");
    let tests = ok(
        w.mcp(&w.alex, "joppa_tests", json!({"ac": A})).await?,
        "joppa_tests",
    )?;
    ensure!(
        tests["tests"][0]["address"] == TEST && tests["tests_revision"] == 3,
        "{tests}"
    );
    Ok(())
}

// clarity point 7
#[tokio::test]
async fn mcp_tests_without_a_criterion_give_the_same_overview_as_http() -> Result<()> {
    let w = world(&[TEST]).await?;
    w.recorded(
        "0123456789abcdef0123",
        json!([{"test":TEST,"status":"pass"}]),
    )
    .await?;
    let mcp = ok(
        w.mcp(&w.alex, "joppa_tests", json!({})).await?,
        "joppa_tests",
    )?;
    let http: Value = w
        .http
        .get(format!("{}/api/workspaces/{}/tests", w.root, w.ws))
        .bearer_auth(&w.alex)
        .send()
        .await?
        .json()
        .await?;
    // MCP gives the same items as one page of 25 with the total.
    ensure!(
        mcp["items"] == http["items"] && mcp["total"] == 1 && mcp["items"][0]["address"] == A,
        "{mcp} {http}"
    );
    Ok(())
}

// clarity point 8
#[tokio::test]
async fn fields_and_summary_make_large_list_replies_small() -> Result<()> {
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
    let summary = ok(
        w.mcp(&w.bot, "joppa_tests", json!({"ac":A,"summary":true}))
            .await?,
        "summary",
    )?;
    ensure!(
        summary.get("tests").is_none() && summary["counts"] == json!({"not_run":1}),
        "{summary}"
    );
    Ok(())
}
