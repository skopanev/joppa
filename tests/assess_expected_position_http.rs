//! requirement:readiness-per-criterion over a real server: `assess` with expected_position
//! writes the mark only when the mark that the writer read is still current.
#[path = "common/generic_world.rs"]
mod generic_world;
use anyhow::{Result, ensure};
use generic_world::{World, world};
use joppa::commands::STALE_ASSESSMENT;
use serde_json::{Value, json};

/// The readiness mark of ac:a.
async fn mark(w: &World) -> Result<Value> {
    Ok(w.object("ac:a").await?["process"]["assessment"].clone())
}

/// An assess of ac:a, with expected_position when it is given.
fn assess(ready: bool, reason: &str, expected: Option<&Value>) -> Value {
    let mut command = json!({"op":"assess","target":"ac:a","ready":ready,"reason":reason});
    if let Some(position) = expected {
        command["expected_position"] = position.clone();
    }
    command
}

// readiness-per-criterion-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_assess_with_an_old_expected_position_keeps_the_newer_mark() -> Result<()> {
    let w = world(&[]).await?;
    // A node without a mark has no position: only 0 matches.
    let none = w
        .command(&w.bot, assess(false, "Retry 1", Some(&json!(1))))
        .await?;
    ensure!(none == Err(STALE_ASSESSMENT.into()), "{none:?}");
    w.command(&w.bot, assess(false, "Retry 1", Some(&json!(0))))
        .await?
        .map_err(anyhow::Error::msg)?;
    let first = mark(&w).await?;
    ensure!(
        first["state"] == "waiting"
            && first["actor"] == "bot"
            && first["position"].as_u64() > Some(0),
        "{first}"
    );
    // The operator blocks the node after the bot read the first mark.
    w.command(&w.owner, assess(false, "Operator block", None))
        .await?
        .map_err(anyhow::Error::msg)?;
    let block = mark(&w).await?;
    ensure!(
        block["position"].as_u64() > first["position"].as_u64(),
        "{block}"
    );
    let stale = w
        .command(
            &w.bot,
            assess(true, "Retry passed", Some(&first["position"])),
        )
        .await?;
    ensure!(stale == Err(STALE_ASSESSMENT.into()), "{stale:?}");
    let zero = w
        .command(&w.bot, assess(true, "Retry passed", Some(&json!(0))))
        .await?;
    ensure!(zero == Err(STALE_ASSESSMENT.into()), "{zero:?}");
    // In a batch the refusal stops every operation: the new node is not saved.
    let task = json!({"op":"save_object","kind":"task","id":"retry","revision":0,"parent":"ac:a",
        "data":{"title":"Retry","owner":"owner","body":"Retry the run"},"reason":"Plan the retry"});
    let operations = json!([{"command":task},{"command":assess(true, "Retry passed", Some(&first["position"]))}]);
    let batch = json!({"request_id":uuid::Uuid::new_v4().to_string(),"operations":operations});
    let refused = w.mcp(&w.bot, "joppa_batch", batch).await?;
    ensure!(
        refused
            .as_ref()
            .is_err_and(|e| e.contains(STALE_ASSESSMENT)),
        "{refused:?}"
    );
    let read = w
        .mcp(&w.bot, "joppa_objects", json!({"address":"task:retry"}))
        .await?;
    ensure!(read.is_err(), "the batch saved no node: {read:?}");
    ensure!(mark(&w).await? == block, "the operator block stays");
    // With the position of the newest mark the write passes.
    w.command(
        &w.bot,
        assess(true, "Operator answered", Some(&block["position"])),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let ready = mark(&w).await?;
    ensure!(
        ready["state"] == "ready"
            && ready["reason"] == "Operator answered"
            && ready["position"].as_u64() > block["position"].as_u64(),
        "{ready}"
    );
    Ok(())
}
