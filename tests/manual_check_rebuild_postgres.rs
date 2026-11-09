//! requirement:jjhid61n: a criterion with a Task child and no test node takes the tone and
//! the label of its manual check, also when store::backfill_board rebuilds every projection,
//! and its requirement takes the same tone.
#[path = "common/manual_check.rs"]
mod manual_check;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use manual_check::{check, day};
use serde_json::{Value, json};

/// The tone and the label of A, and the tone of R, from joppa_objects.
async fn read(w: &record_world::World) -> Result<(Value, Value, Value)> {
    let a = w.object("ac:a").await?;
    let r = manual_check::tone(w, "requirement:r").await?;
    Ok((a["status"]["tone"].clone(), a["status"]["label"].clone(), r))
}

/// Make the board row of the workspace stale and rebuild every projection with
/// store::backfill_board.
async fn rebuild(w: &record_world::World) -> Result<()> {
    let stale = "UPDATE joppa.board_cache SET first_view=first_view||'{\"projection_version\":\"0\"}'::jsonb WHERE workspace=$1";
    let rows = w.pool.get().await?.execute(stale, &[&w.ws]).await?;
    ensure!(rows == 1, "no board row of {}", w.ws);
    let rebuilt = joppa::store::backfill_board(&w.pool).await?;
    ensure!(rebuilt >= 1, "backfill_board rebuilt no workspace");
    Ok(())
}

// ac:nd3rb6a1
// test:n68ypuuj
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_manual_check_keeps_its_tone_and_label_when_the_board_is_rebuilt() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let revision = tree.map_err(anyhow::Error::msg)?["schema"]["revision"].clone();
    let task = json!({"title":"Tasks","role":"task","status":"none","parents":["ac"]});
    let schema = json!({"op":"set_tree_schema","revision":revision,"put_types":{"task":task},
        "reason":"Add tasks under criteria"});
    w.command(&w.owner, schema)
        .await?
        .map_err(anyhow::Error::msg)?;
    let child = json!({"op":"save_object","kind":"task","revision":0,"parent":"ac:a",
        "data":{"title":"Check A by hand","owner":"owner"},"reason":"Create the task"});
    w.command(&w.bot, child)
        .await?
        .map_err(anyhow::Error::msg)?;
    for (fields, tone, label) in [
        (
            json!({"check_result":"pass"}),
            "green",
            "Manual check passed",
        ),
        (json!({"check_result":"fail"}), "red", "Manual check failed"),
        (
            json!({"check_result":"pass","valid_until":day(0)}),
            "grey",
            "Manual check expired",
        ),
    ] {
        check(&w, "ac:a", fields.clone()).await?;
        // The Task child does not change the tone of A, and R has the tone of A.
        for step in ["the attach", "the rebuild"] {
            if step == "the rebuild" {
                rebuild(&w).await?;
            }
            let (a, a_label, r) = read(&w).await?;
            ensure!(
                a == tone && a_label == label && r == tone,
                "after {step} {fields}: A must be {tone} {label:?} and R {tone}: A {a} {a_label}, R {r}"
            );
        }
    }
    Ok(())
}
