//! requirement:mcp-tools over a real server: the defaults of joppa_batch complete each
//! save_object that omits a field, and a command keeps each field that it gives.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

const QUESTION: &str = "Which condition does the criterion check?";

/// The owner and the revision reason of a current object.
async fn saved(w: &record_world::World, address: &str) -> Result<(String, String)> {
    let row = w.pool.get().await?.query_one("SELECT document->'data'->>'owner', document->>'reason' FROM joppa.objects WHERE workspace=$1 AND address=$2", &[&w.ws, &address]).await?;
    Ok((row.get(0), row.get(1)))
}

fn save(title: &str, extra: Value) -> Value {
    let mut command = json!({"op":"save_object","kind":"ac","revision":0,
        "parent":"requirement:r","data":{"title":title}});
    for (key, value) in extra.as_object().into_iter().flatten() {
        match key.as_str() {
            "owner" => command["data"]["owner"] = value.clone(),
            _ => command[key] = value.clone(),
        }
    }
    command
}

// ac:48gvooxf
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_batch_default_completes_each_save_that_omits_the_field() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let revision = tree.map_err(anyhow::Error::msg)?["schema"]["revision"].clone();
    let ac = json!({"title":"Criteria","role":"ac","status":"children",
        "parents":["requirement"],"question":QUESTION});
    w.command(
        &w.owner,
        json!({"op":"set_tree_schema","revision":revision,
        "put_types":{"ac":ac},"reason":"Ask a question"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let note = json!({"op":"note","target":"ac:a","id":"plain","text":"A comment"});
    let defaults = json!({"reason":"Default reason","question_checked":QUESTION,
        "data":{"owner":"alex"}});
    let operations = json!([
        {"ref":"one","command":save("One", json!({}))},
        {"ref":"two","command":save("Two", json!({"owner":"owner","reason":"Own reason"}))},
        {"command":note}
    ]);
    let batch = json!({"request_id":"defaults-1","defaults":defaults,"operations":operations});
    let reply = w.mcp(&w.bot, "joppa_batch", batch.clone()).await?;
    let reply = reply.map_err(anyhow::Error::msg)?;
    let address = |i: usize| {
        reply["results"][i]["object"]["address"]
            .as_str()
            .unwrap_or("")
    };
    let (one, two) = (address(0).to_owned(), address(1).to_owned());
    ensure!(
        saved(&w, &one).await? == ("alex".into(), "Default reason".into()),
        "{reply}"
    );
    ensure!(saved(&w, &two).await? == ("owner".into(), "Own reason".into()));
    let stored: String = w
        .pool
        .get()
        .await?
        .query_one(
            "SELECT command::text FROM joppa.events WHERE workspace=$1 AND command->>'op'='note'",
            &[&w.ws],
        )
        .await?
        .get(0);
    // The note takes no default field.
    let stored: Value = serde_json::from_str(&stored)?;
    ensure!(
        ["reason", "question_checked", "data"]
            .iter()
            .all(|key| stored.get(key).is_none())
            && stored["text"] == note["text"],
        "{stored}"
    );
    // An identical retry gives the same receipt.
    let again = w.mcp(&w.bot, "joppa_batch", batch).await?;
    ensure!(again.map_err(anyhow::Error::msg)?["results"][0]["object"]["address"] == one.as_str());
    let before = w.position().await?;
    for (id, defaults) in [
        ("defaults-op", json!({"op":"note"})),
        ("defaults-data", json!({"data":"alex"})),
    ] {
        let batch = json!({"request_id":id,"defaults":defaults,
            "operations":[{"command":save("Three", json!({}))}]});
        let refused = w.mcp(&w.bot, "joppa_batch", batch).await?;
        ensure!(refused.is_err(), "{id}: {refused:?}");
    }
    ensure!(w.position().await? == before, "no operation appends");
    Ok(())
}
