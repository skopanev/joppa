//! requirement:edit-makes-revision over a real server: an update of save_object merges its
//! data by key and keeps an omitted parent and links, and clean: true replaces the data.
#[path = "common/generic_world.rs"]
mod generic_world;
use anyhow::{Result, ensure};
use generic_world::{World, world};
use serde_json::{Value, json};

/// An update of ac:b with `fields` next to op, kind, id, revision and reason.
async fn update(w: &World, revision: u32, fields: Value) -> Result<Result<Value, String>> {
    let mut command =
        json!({"op":"save_object","kind":"ac","id":"b","revision":revision,"reason":"Edit B"});
    for (key, value) in fields.as_object().into_iter().flatten() {
        command[key] = value.clone();
    }
    w.command(&w.bot, command).await
}

/// The data, the parent and the link targets of ac:b.
async fn read(w: &World) -> Result<(Value, Value, Vec<Value>)> {
    let b = w.object("ac:b").await?;
    let links = b["links"].as_array().cloned().unwrap_or_default();
    let targets = links.iter().map(|l| l["target"].clone()).collect();
    Ok((b["data"].clone(), b["parent"].clone(), targets))
}

// ac:ib67e9du
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_update_merges_data_by_key_and_clean_replaces_it() -> Result<()> {
    let w = world(&[]).await?;
    let create = json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":"requirement:r",
        "data":{"title":"B","owner":"owner","body":"The rule","env":"dev"},
        "links":[{"target":"ac:a","relation":"related"}],"reason":"Create B"});
    w.command(&w.bot, create)
        .await?
        .map_err(anyhow::Error::msg)?;
    update(&w, 1, json!({"data":{"env":"prod"}}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let (data, parent, links) = read(&w).await?;
    ensure!(
        data["title"] == "B" && data["body"] == "The rule" && data["env"] == "prod",
        "merge: {data}"
    );
    ensure!(
        parent == "requirement:r" && links == [json!("ac:a")],
        "{parent} {links:?}"
    );
    // The journal names the whole data and the parent of the update.
    let sql = "SELECT command::text FROM joppa.events WHERE workspace=$1 AND command->>'op'='save_object' AND command->>'id'='b' ORDER BY position DESC LIMIT 1";
    let row = w.pool.get().await?.query_one(sql, &[&w.ws]).await?;
    let event: Value = serde_json::from_str(row.get(0))?;
    ensure!(
        event["data"]["body"] == "The rule" && event["parent"] == "requirement:r",
        "{event}"
    );
    update(&w, 2, json!({"data":{"env":null}}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let (data, _, _) = read(&w).await?;
    ensure!(
        data.get("env").is_none() && data["body"] == "The rule",
        "null: {data}"
    );
    // The server fills the default fields of the type, such as an empty body.
    let clean = json!({"clean":true,"data":{"title":"B","owner":"owner"}});
    update(&w, 3, clean).await?.map_err(anyhow::Error::msg)?;
    let (data, parent, _) = read(&w).await?;
    ensure!(
        data["body"] != "The rule" && data.get("env").is_none() && data["title"] == "B",
        "clean: {data}"
    );
    ensure!(parent == "requirement:r", "clean parent: {parent}");
    let refused = update(&w, 4, json!({"clean":"yes","data":{"env":"dev"}})).await?;
    ensure!(
        refused.is_err_and(|text| text.contains(joppa::save_merge::CLEAN_REFUSAL)),
        "clean yes"
    );
    let (data, _, _) = read(&w).await?;
    ensure!(
        data.get("env").is_none(),
        "a refused save changed data: {data}"
    );
    Ok(())
}
