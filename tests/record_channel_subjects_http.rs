//! One subject per test over a real server: member bot records outcomes through the MCP
//! tool joppa_record_results, and a member reads GET /tests?ac=A.
use anyhow::{Result, ensure};
use serde_json::json;

#[path = "common/record_world.rs"]
mod record_world;
use record_world::{test, world};

const X: &str = "tests/record.rs > x";
const Y: &str = "tests/record.rs > y";

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_record_updates_only_the_tests_it_names() -> Result<()> {
    // no-mixed-subjects-ac1: X and Y pass on C1, then a record on C2 names only X.
    let w = world(&[X, Y]).await?;
    let (c1, c2) = ("c1c1c1c", "c2c2c2c");
    w.recorded(
        c1,
        json!([{"test":X,"status":"pass"},{"test":Y,"status":"pass"}]),
    )
    .await?;
    w.recorded(c2, json!([{"test":X,"status":"pass"}])).await?;
    let s = w.tests_http(&w.bot).await?;
    ensure!(s["status"] == "green", "{s}");
    ensure!(
        test(&s, X)["commit"] == c2 && test(&s, Y)["commit"] == c1,
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_failure_beats_a_later_pass_on_the_same_commit() -> Result<()> {
    // no-mixed-subjects-ac2: on commit C, X is recorded as fail and then as pass.
    let w = world(&[X]).await?;
    let c = "c0c0c0c";
    w.recorded(c, json!([{"test":X,"status":"fail","message":"boom"}]))
        .await?;
    w.recorded(c, json!([{"test":X,"status":"pass"}])).await?;
    let s = w.tests_http(&w.bot).await?;
    let x = test(&s, X);
    ensure!(x["status"] == "fail" && x["commit"] == c, "{s}");
    ensure!(s["status"] == "red", "{s}");
    Ok(())
}
