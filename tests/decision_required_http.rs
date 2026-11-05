//! requirement:readiness-per-criterion over a real server: an open question on a node is its
//! readiness mark. A save with data.decision_required is refused, the journal has one
//! migrate_decision_required event, and a read has no question key.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use joppa::decision_migration::REFUSAL;
use record_world::{World, world};
use serde_json::json;

/// The migrate_decision_required events in the journal of the workspace.
async fn migrations(w: &World) -> Result<i64> {
    let sql = "SELECT count(*) FROM joppa.events WHERE workspace=$1 \
        AND command->>'op'='migrate_decision_required'";
    Ok(w.pool.get().await?.query_one(sql, &[&w.ws]).await?.get(0))
}

// readiness-per-criterion-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_open_question_is_a_waiting_mark_and_the_decision_flag_is_refused() -> Result<()> {
    let w = world(&[]).await?;
    let a = w.object("ac:a").await?;
    ensure!(a["data"].get("decision_required").is_none(), "{a}");
    let mut data = a["data"].clone();
    data["decision_required"] = json!(true);
    let save = json!({"op":"save_object","kind":"ac","id":"a","revision":a["revision"],
        "parent":"requirement:r","data":data,"reason":"Ask the owner"});
    let refused = w.command(&w.bot, save).await?;
    ensure!(refused == Err(REFUSAL.to_owned()), "{refused:?}");
    // An older answer request in the document gives no question key.
    let older = json!({"op":"save_object","kind":"requirement","id":"ask","revision":0,
        "parent":null,"data":{"title":"Ask the owner","owner":"owner","body":"The limit rule",
        "decision":{"outcome":"needs_information","asked":"alex","question":"Which limit?"}},
        "reason":"Record the question"});
    w.command(&w.bot, older)
        .await?
        .map_err(anyhow::Error::msg)?;
    let read = w.object("requirement:ask").await?;
    ensure!(read.get("question").is_none(), "{read}");
    let question = "OPEN QUESTION: Which limit applies?";
    let assess = json!({"op":"assess","target":"ac:a","ready":false,"reason":question});
    w.command(&w.bot, assess)
        .await?
        .map_err(anyhow::Error::msg)?;
    let page = w
        .mcp(&w.bot, "joppa_objects", json!({"parent":"*","ready":false}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let listed = page["items"].as_array().cloned().unwrap_or_default();
    ensure!(listed.iter().any(|i| i["address"] == "ac:a"), "{page}");
    let mark = &w.object("ac:a").await?["process"]["assessment"];
    ensure!(
        mark["state"] == "waiting" && mark["reason"] == question,
        "{mark}"
    );
    ensure!(migrations(&w).await? == 1, "one migration event");
    Ok(())
}
