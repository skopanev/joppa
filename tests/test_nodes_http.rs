//! requirement:test-nodes over a real server: a test is a node under its criterion, its latest
//! result decides its tone, and one rollup decides every level: red, orange, yellow, grey, green.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

const ONE: &str = "tests/x.rs > one";
const TWO: &str = "tests/x.rs > two";

async fn tones(w: &record_world::World, args: Value) -> Result<Vec<(String, String)>> {
    let list = w
        .mcp(&w.bot, "joppa_objects", args)
        .await?
        .map_err(anyhow::Error::msg)?;
    let items = list["items"].as_array().cloned().unwrap_or_default();
    Ok(items
        .iter()
        .map(|i| {
            (
                i["title"].as_str().unwrap_or_default().into(),
                i["status"].as_str().unwrap_or_default().into(),
            )
        })
        .collect())
}

async fn tone(w: &record_world::World, address: &str) -> Result<String> {
    Ok(w.object(address).await?["status"]["tone"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

// test-nodes-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_test_is_a_node_and_its_latest_result_decides_its_tone() -> Result<()> {
    let w = record_world::world(&[ONE, TWO]).await?;
    let nodes = json!({"parent":"ac:a","kind":"test","fields":["title","status"]});
    ensure!(
        tones(&w, nodes.clone()).await?
            == [(ONE.into(), "grey".into()), (TWO.into(), "grey".into())]
            || tones(&w, nodes.clone()).await?
                == [(TWO.into(), "grey".into()), (ONE.into(), "grey".into())]
    );
    let first = w.recorded(&"a".repeat(40), json!([{"test":ONE,"status":"pass"},{"test":TWO,"status":"fail"},{"test":"tests/x.rs > free","status":"pass"}])).await?;
    ensure!(first["unbound"] == json!(["tests/x.rs > free"]), "{first}");
    let mut red = tones(&w, nodes.clone()).await?;
    red.sort();
    ensure!(
        red == [(ONE.into(), "green".into()), (TWO.into(), "red".into())],
        "{red:?}"
    );
    ensure!(tone(&w, "ac:a").await? == "red" && tone(&w, "requirement:r").await? == "red");
    w.recorded(&"b".repeat(40), json!([{"test":TWO,"status":"pass"}]))
        .await?;
    ensure!(tone(&w, "ac:a").await? == "green" && tone(&w, "requirement:r").await? == "green");
    // The status filter finds the test nodes by tone.
    let green = tones(&w, json!({"kind":"test","status":"green"})).await?;
    ensure!(green.len() == 2, "{green:?}");
    Ok(())
}

// test-nodes-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn one_rollup_gives_red_orange_yellow_grey_green_on_every_level() -> Result<()> {
    let w = record_world::world(&[ONE]).await?;
    w.recorded(&"a".repeat(40), json!([{"test":ONE,"status":"pass"}]))
        .await?;
    ensure!(tone(&w, "requirement:r").await? == "green");
    // A criterion without a test node is orange, and orange beats green.
    let save = json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":"requirement:r",
        "data":{"title":"B","owner":"owner"},"reason":"Create"});
    w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    ensure!(tone(&w, "ac:b").await? == "orange" && tone(&w, "requirement:r").await? == "orange");
    // An archived node is purple and does not count.
    let archive =
        json!({"op":"archive_object","target":"ac:b","revision":1,"reason":"Out of scope"});
    w.command(&w.owner, archive)
        .await?
        .map_err(anyhow::Error::msg)?;
    ensure!(tone(&w, "ac:b").await? == "purple" && tone(&w, "requirement:r").await? == "green");
    // A node that waits for a dependency is yellow, and yellow beats green above it. The
    // target ac:c is under its own root requirement:q, so it does not count for R.
    let a = w.object("ac:a").await?;
    for command in [
        json!({"op":"save_object","kind":"requirement","id":"q","revision":0,"parent":null,
            "data":{"title":"Q","owner":"owner","body":"The second rule"},"reason":"Create"}),
        json!({"op":"save_object","kind":"ac","id":"c","revision":0,"parent":"requirement:q",
            "data":{"title":"C","owner":"owner"},"reason":"Create"}),
        json!({"op":"save_object","kind":"ac","id":"a","revision":a["revision"],
            "parent":"requirement:r","data":a["data"],"reason":"Order after C",
            "relations":{"depends_on":{"edges":[{"to":"ac:c"}]}}}),
    ] {
        w.command(&w.bot, command)
            .await?
            .map_err(anyhow::Error::msg)?;
    }
    let pair = (tone(&w, "ac:a").await?, tone(&w, "requirement:r").await?);
    ensure!(pair == ("yellow".into(), "yellow".into()), "{pair:?}");
    // A failure beats every other tone.
    w.recorded(&"b".repeat(40), json!([{"test":ONE,"status":"fail"}]))
        .await?;
    ensure!(tone(&w, "ac:a").await? == "red" && tone(&w, "requirement:r").await? == "red");
    Ok(())
}

// test-nodes-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_old_test_path_is_refused_and_names_its_replacement() -> Result<()> {
    let w = record_world::world(&[ONE]).await?;
    let a = w.object("ac:a").await?;
    let mut data = a["data"].clone();
    data["tests"] = json!([ONE]);
    for (command, error) in [
        (
            json!({"op":"save_object","kind":"ac","id":"a","revision":a["revision"],"parent":"requirement:r","data":data,"reason":"List"}),
            "use save_object with kind test",
        ),
        (
            json!({"op":"set_ac_tests","target":"ac:a","tests":[ONE],"reason":"List"}),
            "set_ac_tests was removed; use save_object with kind test",
        ),
    ] {
        let reply = w.command(&w.bot, command.clone()).await?;
        ensure!(
            reply.as_ref().err().is_some_and(|e| e.starts_with(error)),
            "{command}: {reply:?}"
        );
    }
    // No name rule is left: a title without a file saves.
    let saved = w
        .command(
            &w.bot,
            json!({"op":"save_object","kind":"test","revision":0,"parent":"ac:a",
        "data":{"title":"no file","owner":"owner"},"reason":"Bind"}),
        )
        .await?;
    ensure!(saved.is_ok(), "{saved:?}");
    let tool = w.mcp(&w.bot, "joppa_tests", json!({"ac":"ac:a"})).await?;
    ensure!(
        tool.as_ref().err().is_some_and(
            |e| e.starts_with("joppa_tests was removed; use joppa_objects with kind test")
        ),
        "{tool:?}"
    );
    let route = w
        .http
        .get(format!("{}/api/workspaces/{}/tests?ac=ac:a", w.root, w.ws))
        .bearer_auth(&w.bot)
        .send()
        .await?;
    ensure!(route.status() == 404, "{}", route.status());
    Ok(())
}
