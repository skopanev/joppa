//! requirement:jjhid61n over a real server: the latest live manual check after the boundary
//! decides the status of a criterion without test nodes. A criterion with a test node takes
//! its status only from its tests.
#[path = "common/manual_check.rs"]
mod manual_check;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use manual_check::{check, day, expect};
use serde_json::json;

const GREY_NO_TESTS: Option<&str> = Some("no tests");
const AWAITS: Option<&str> = Some("Awaits checks");

/// A is `a` with the reason `why`, B is grey, and R is `r` after `step`. A criterion without
/// test nodes and without checks has the tone orange (No children) on the board, so grey
/// "no tests" accepts the tone orange for A and R.
async fn step(
    w: &record_world::World,
    step: &str,
    a: &str,
    why: Option<&str>,
    r: &str,
) -> Result<()> {
    let tones = |want: &str| match why {
        Some("no tests") => vec![want.to_owned(), "orange".to_owned()],
        _ => vec![want.to_owned()],
    };
    let (status, reason, tone) = manual_check::state(w, "ac:a").await?;
    ensure!(
        status == a && why.is_none_or(|y| reason == y) && tones(a).iter().any(|t| tone == *t),
        "{step}: A must be {a} {why:?}: status {status}, reason {reason}, tone {tone}"
    );
    expect(w, step, "ac:b", "grey", None).await?;
    let tone = manual_check::tone(w, "requirement:r").await?;
    ensure!(
        tones(r).iter().any(|t| tone == *t),
        "{step}: R must be {r}: tone {tone}"
    );
    Ok(())
}

// ac:mnecdzab
// test:TBD
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_latest_manual_check_decides_a_criterion_without_test_nodes() -> Result<()> {
    let w = record_world::world(&[]).await?;
    manual_check::criterion(&w, "b", &["tests/manual.rs > b"]).await?;
    let tomorrow = day(1);
    check(&w, "ac:a", json!({})).await?;
    step(
        &w,
        "a file without check_result",
        "grey",
        GREY_NO_TESTS,
        "grey",
    )
    .await?;
    check(&w, "ac:a", json!({"check_result":"pass"})).await?;
    step(&w, "pass", "green", None, "grey").await?;
    let fail = json!({"check_result":"fail","valid_until":tomorrow});
    check(&w, "ac:a", fail).await?;
    step(&w, "fail with valid_until tomorrow", "red", None, "red").await?;
    let pass = json!({"check_result":"pass","valid_until":tomorrow});
    let last = check(&w, "ac:a", pass).await?;
    step(&w, "pass with valid_until tomorrow", "green", None, "grey").await?;
    let id = last["file"]["id"].as_str().context("file id")?;
    let detach = json!({"op":"detach","target":"ac:a","id":id,"reason":"Drop the check"});
    w.command(&w.bot, detach)
        .await?
        .map_err(anyhow::Error::msg)?;
    step(&w, "the detach of the last file", "red", None, "red").await?;
    manual_check::new_body(&w, "ac:a", "The new criterion text").await?;
    step(&w, "the new body of A", "grey", AWAITS, "grey").await?;
    // One batch: an attach of pass, then a new body. The file has the time of the boundary.
    let file = &last["file"];
    let a = w.object("ac:a").await?;
    let operations = json!([
        {"command":{"op":"attach","target":"ac:a","id":"batch-pass","name":"check.txt",
            "content_type":"text/plain","size":file["size"],"hash":file["hash"],"check_result":"pass"}},
        {"command":{"op":"save_object","kind":"ac","id":"a","revision":a["revision"],
            "data":{"body":"The criterion text of the batch"},"reason":"Change the text"}}
    ]);
    let batch = json!({"request_id":uuid::Uuid::new_v4().to_string(),"operations":operations});
    w.mcp(&w.bot, "joppa_batch", batch)
        .await?
        .map_err(|e| anyhow::anyhow!("joppa_batch: {e}"))?;
    step(&w, "the batch", "grey", AWAITS, "grey").await?;
    check(&w, "ac:a", json!({"check_result":"pass"})).await?;
    step(&w, "pass after the batch", "green", None, "grey").await?;
    // The test node of B decides: a pass on B changes nothing.
    check(&w, "ac:b", json!({"check_result":"pass"})).await?;
    step(&w, "pass on B", "green", None, "grey").await?;
    Ok(())
}
