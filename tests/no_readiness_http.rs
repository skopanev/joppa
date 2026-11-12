//! Readiness is gone over a real server. No read gives work_ready or waits_for, a ready filter
//! or field is refused, and a depends_on edge to a blue criterion leaves its source green.
//! Yellow means only an Inbox input that waits for processing. The fixture: requirement:r >
//! ac:a, blue, with one test node, and requirement:q > ac:b with a depends_on edge to ac:a.
#[path = "common/blue_world.rs"]
mod blue_world;
#[path = "common/plain_http.rs"]
mod plain_http;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use plain_http::{declares, get_refused, tools};
use record_world::{A, World};
use relations_world::{edges, msg, refused, saved, stored, to, tone, without_key};
use serde_json::{Value, json};

/// The test of criterion A. It has no result, so A is blue.
const TA: &str = "tests/no_readiness.rs > a";
/// The test of criterion B.
const TB: &str = "tests/no_readiness.rs > b";
const B: &str = "ac:b";

/// The world with A blue, and requirement:q > ac:b with a depends_on edge to A.
async fn dependent() -> Result<World> {
    let w = relations_world::world(&[TA]).await?;
    let blue = tone(&w, A).await?;
    ensure!(blue == "blue", "A is blue: {blue}");
    saved(&w, "requirement:q", 0, json!({})).await?;
    let to_a = json!({"depends_on":{"edges":[{"to":A}]}});
    saved(&w, B, 0, json!({"parent":"requirement:q","relations":to_a})).await?;
    Ok(w)
}

/// A reply has neither the key work_ready nor the key waits_for at any depth.
fn no_readiness(what: &str, reply: &Value) -> Result<()> {
    let clean = without_key(reply, "work_ready") && without_key(reply, "waits_for");
    ensure!(clean, "{what} gives readiness: {reply}");
    Ok(())
}

/// The tone and the label of a detail read: MCP gives status, HTTP gives signal.
fn tone_label(read: &Value) -> (Value, Value) {
    let s = if read["status"].is_object() {
        &read["status"]
    } else {
        &read["signal"]
    };
    (s["tone"].clone(), s["label"].clone())
}

/// The MCP and the HTTP detail reads of `address`.
async fn both(w: &World, address: &str) -> Result<[(String, Value); 2]> {
    let mcp = w.object(address).await?;
    let path = format!("/objects?address={address}");
    let http = blue_world::get(w, &w.bot, &path).await?;
    Ok([
        (format!("joppa_objects {address}"), mcp),
        (format!("GET {path}"), http),
    ])
}

// ac:q2qmqne7
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn no_read_gives_readiness_and_a_ready_filter_or_field_is_refused() -> Result<()> {
    let w = dependent().await?;
    saved(&w, "task:t", 0, json!({"parent":B})).await?;
    let tree = json!({"parent":"*","depth":100,"limit":25});
    let page = w.mcp(&w.bot, "joppa_objects", tree).await?.map_err(msg)?;
    no_readiness("joppa_objects parent *", &page)?;
    let listed = plain_http::addresses(&page);
    let all = [B, "task:t"].iter().all(|a| listed.iter().any(|l| l == a));
    ensure!(all, "the list has ac:b and task:t: {page}");
    let found = json!({"query":"fixture","limit":25});
    let search = w.mcp(&w.bot, "joppa_search", found).await?.map_err(msg)?;
    no_readiness("joppa_search", &search)?;
    let list = blue_world::get(&w, &w.bot, "/objects?parent=*&depth=100&limit=25").await?;
    no_readiness("GET /objects", &list)?;
    no_readiness("joppa_objects task:t", &w.object("task:t").await?)?;
    for (what, read) in both(&w, B).await? {
        no_readiness(&what, &read)?;
        let kept = edges(&read, "depends_on");
        ensure!(kept == [to(A)], "{what} keeps depends_on to A: {read}");
    }
    let removed = "the ready filter was removed";
    let reply = w
        .mcp(&w.bot, "joppa_objects", json!({"ready":false}))
        .await?;
    refused(reply, removed)?;
    let args = json!({"query":"fixture","ready":false});
    refused(w.mcp(&w.bot, "joppa_search", args).await?, removed)?;
    get_refused(&w, &w.bot, "/objects?ready=false", removed).await?;
    for field in ["work_ready", "waits_for"] {
        let args = json!({"address":B,"fields":[field]});
        let reply = w.mcp(&w.bot, "joppa_objects", args).await?;
        refused(reply, &format!("{field} was removed"))?;
    }
    for tool in tools(&w, &w.bot).await? {
        ensure!(!declares(&tool, "ready"), "a ready property: {tool}");
    }
    Ok(())
}

// ac:kk7epj2h
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn yellow_means_only_an_inbox_input_that_waits_for_processing() -> Result<()> {
    let w = dependent().await?;
    saved(&w, "test:b", 0, json!({"parent":B,"title":TB})).await?;
    w.recorded(&"b".repeat(40), json!([{"test":TB,"status":"pass"}]))
        .await?;
    saved(&w, "note:i", 0, json!({})).await?;
    for address in [B, "requirement:q"] {
        for (what, read) in both(&w, address).await? {
            let (found, _) = tone_label(&read);
            ensure!(found == "green", "{what} is green: {read}");
        }
    }
    for (what, read) in both(&w, "note:i").await? {
        let found = tone_label(&read);
        let wanted = (json!("yellow"), json!("Input awaits processing"));
        ensure!(found == wanted, "{what}: {found:?} {read}");
    }
    let revision = stored(&w, "note:i").await?.0;
    let archive = json!({"op":"archive_object","target":"note:i","revision":revision,
        "reason":"Processed into the tree"});
    w.command(&w.bot, archive).await?.map_err(msg)?;
    for (what, read) in both(&w, "note:i").await? {
        let (found, _) = tone_label(&read);
        ensure!(
            found == "purple",
            "{what} is purple after the archive: {read}"
        );
    }
    Ok(())
}
