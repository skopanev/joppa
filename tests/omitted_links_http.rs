//! requirement:edit-makes-revision over a real server: a save_object without links keeps the
//! links of the object, and links: [] removes them.
#[path = "common/generic_world.rs"]
mod generic_world;
use anyhow::{Result, ensure};
use generic_world::{World, world};
use serde_json::{Value, json};

/// The link targets and the revision of ac:b.
async fn read(w: &World) -> Result<(Vec<String>, Value)> {
    let b = w.object("ac:b").await?;
    let links = b["links"].as_array().cloned().unwrap_or_default();
    let targets = links
        .iter()
        .filter_map(|l| l["target"].as_str().map(String::from));
    Ok((targets.collect(), b["revision"].clone()))
}

/// A save of ac:b with `title`, and with `links` when they are given.
fn save(revision: u32, title: &str, links: Option<Value>) -> Value {
    let mut command = json!({"op":"save_object","kind":"ac","id":"b","revision":revision,
        "parent":"requirement:r","data":{"title":title,"owner":"owner"},"reason":"Edit B"});
    if let Some(links) = links {
        command["links"] = links;
    }
    command
}

// edit-makes-revision-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_save_without_links_keeps_them_and_an_empty_list_removes_them() -> Result<()> {
    let w = world(&[]).await?;
    let link = json!([{"target":"ac:a","relation":"related"}]);
    for command in [save(0, "B", Some(link)), save(1, "B again", None)] {
        w.command(&w.bot, command)
            .await?
            .map_err(anyhow::Error::msg)?;
    }
    let (targets, revision) = read(&w).await?;
    ensure!(
        targets == ["ac:a"] && revision == 2,
        "{targets:?} {revision}"
    );
    w.command(&w.bot, save(2, "B alone", Some(json!([]))))
        .await?
        .map_err(anyhow::Error::msg)?;
    let (targets, revision) = read(&w).await?;
    ensure!(
        targets.is_empty() && revision == 3,
        "{targets:?} {revision}"
    );
    Ok(())
}
