//! requirement:q438uw0s over a real server: the links status rule and from: links ignore flows,
//! and an old decide_note event adds a related edge also when a flow edge exists.
#[path = "common/legacy_event.rs"]
mod legacy_event;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/relations_world.rs"]
mod relations_world;
use anyhow::{Result, ensure};
use relations_world::{change, declare, edges, msg, node, saved, schema, stored, to, tone};
use serde_json::json;

const TEST: &str = "tests/relations_status_http.rs > ";

/// The typed field f with the values high and low and the source `from`.
fn field(from: &str) -> serde_json::Value {
    json!({"f":{"type":"enum","values":["high","low"],"from":from}})
}

// ac:loecwtbg
// test:go9ltybt
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_links_status_rule_and_from_links_ignore_flows() -> Result<()> {
    let w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap")]).await?;
    let ac = node(
        "Criteria",
        "ac",
        &["requirement"],
        json!({"status":"children","fields":field("own")}),
    );
    let area = node(
        "Areas",
        "document",
        &[],
        json!({"status":"links","fields":field("links")}),
    );
    schema(&w, json!({"put_types":{"ac":ac,"area":area}}))
        .await?
        .map_err(msg)?;
    saved(&w, "area:r", 0, json!({})).await?;
    // N is red with f high, G is green with f low.
    for (id, f) in [("n", "high"), ("g", "low")] {
        let address = format!("ac:{id}");
        saved(&w, &address, 0, json!({"parent":"requirement:r","f":f})).await?;
        let test = json!({"op":"save_object","kind":"test","revision":0,"parent":address,
            "data":{"title":format!("{TEST}{id}"),"owner":"owner"},"reason":"Bind the test"});
        w.command(&w.bot, test).await?.map_err(msg)?;
    }
    let result = |id: &str, status: &str| json!({"test":format!("{TEST}{id}"),"status":status});
    w.recorded("5a5a5a4", json!([result("n", "fail"), result("g", "pass")]))
        .await?;
    ensure!(
        tone(&w, "ac:n").await? == "red" && tone(&w, "ac:g").await? == "green",
        "setup"
    );
    let before = tone(&w, "area:r").await?;
    change(
        &w,
        "ac:n",
        json!({"relations":{"swap":{"edges":[{"to":"area:r"}]}}}),
    )
    .await?;
    let r = w.object("area:r").await?;
    ensure!(
        r["status"]["tone"] == before,
        "R keeps its tone {before}: {r}"
    );
    ensure!(
        r["data"].get("f").is_none(),
        "R has no f after the swap save: {r}"
    );
    change(
        &w,
        "ac:g",
        json!({"relations":{"related":{"edges":[{"to":"area:r"}]}}}),
    )
    .await?;
    // The new related edge resets the tests of G: a new record passes the test of G.
    w.recorded("5a5a5a5", json!([result("g", "pass")])).await?;
    let r = w.object("area:r").await?;
    ensure!(
        r["status"]["tone"] == "green" && r["data"]["f"] == "low",
        "after the related save and the new record: {r}"
    );
    Ok(())
}

// ac:yq4hkhb7
// test:n5af88w0
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn decide_note_adds_a_related_edge_also_when_a_flow_edge_exists() -> Result<()> {
    let mut w = relations_world::world(&[]).await?;
    declare(&w, &[("swap", "Swap")]).await?;
    let swap = json!({"swap":{"edges":[{"to":"requirement:r"}]}});
    saved(&w, "note:n", 0, json!({"relations":swap})).await?;
    let (revision, ..) = stored(&w, "note:n").await?;
    let decide = json!({"op":"decide_note","target":"note:n","revision":revision,
        "outcome":"changed_spec","reason":"Updated the requirement","links":["requirement:r"]});
    // A live decide_note call is refused with its removed-operation text.
    let refused = w.command(&w.bot, decide.clone()).await?;
    let text = joppa::removed_ops::refusal("decide_note");
    ensure!(
        refused
            .as_ref()
            .err()
            .is_some_and(|e| e.contains("decide_note was removed") && e.contains(&text)),
        "the live call: {refused:?}"
    );
    // The old journal holds the decide_note event. The server replays W at its next start.
    w.stop();
    legacy_event::execute(&w.pool, &w.ws, "bot", decide).await?;
    let stale = "UPDATE joppa.board_cache SET first_view='{}'::jsonb WHERE workspace=$1";
    w.pool.get().await?.execute(stale, &[&w.ws]).await?;
    w.start().await?;
    let n = w.object("note:n").await?;
    ensure!(
        edges(&n, "related") == [to("requirement:r")],
        "the related edge: {n}"
    );
    ensure!(
        edges(&n, "swap") == [to("requirement:r")],
        "the swap edge stays: {n}"
    );
    ensure!(n["data"]["status"] == "processed", "N is processed: {n}");
    Ok(())
}
