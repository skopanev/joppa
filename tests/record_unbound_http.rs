//! joppa_record_results over a real server: the reply names in unbound each test of the record
//! that has no present test node. A test of an archived criterion is unbound.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

const BOUND: &str = "tests/x.rs > bound";
const FREE: &str = "tests/x.rs > free";
const ARCHIVED: &str = "tests/x.rs > archived";

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_record_names_the_tests_that_no_present_criterion_lists() -> Result<()> {
    let w = record_world::world(&[BOUND]).await?;
    // Criterion B has one test node. An object with live children cannot be archived, so the
    // owner archives the test node first and then B.
    for (token, command) in [
        (
            &w.bot,
            json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":"requirement:r",
            "data":{"title":"B","owner":"owner"},"reason":"Create"}),
        ),
        (
            &w.bot,
            json!({"op":"save_object","kind":"test","id":"archived","revision":0,"parent":"ac:b",
            "data":{"title":ARCHIVED,"owner":"owner"},"reason":"Bind"}),
        ),
        (
            &w.owner,
            json!({"op":"archive_object","target":"test:archived","revision":1,"reason":"B is out of scope"}),
        ),
        (
            &w.owner,
            json!({"op":"archive_object","target":"ac:b","revision":1,"reason":"B is out of scope"}),
        ),
    ] {
        w.command(token, command.clone())
            .await?
            .map_err(|e| anyhow::anyhow!("{command}: {e}"))?;
    }
    let run = json!([
        {"test":BOUND,"status":"pass"},{"test":FREE,"status":"pass"},{"test":ARCHIVED,"status":"fail"}
    ]);
    let first = w.recorded(&"a".repeat(40), run.clone()).await?;
    ensure!(first["unbound"] == json!([ARCHIVED, FREE]), "{first}");
    // An identical repeat gives the same names.
    let again = w.recorded(&"a".repeat(40), run.clone()).await?;
    ensure!(
        again["replayed"] == true && again["unbound"] == first["unbound"],
        "{again}"
    );
    // A test node that names the free test binds it. The test of the archived criterion stays
    // unbound.
    let bind = json!({"op":"save_object","kind":"test","revision":0,"parent":"ac:a","data":{"title":FREE,"owner":"owner"},"reason":"Bind"});
    w.command(&w.bot, bind).await?.map_err(anyhow::Error::msg)?;
    let second = w.recorded(&"b".repeat(40), run).await?;
    ensure!(second["unbound"] == json!([ARCHIVED]), "{second}");
    // A record of bound tests only gives an empty list.
    let bound = json!([{"test":BOUND,"status":"pass"},{"test":FREE,"status":"pass"}]);
    let last: Value = w.recorded(&"c".repeat(40), bound).await?;
    ensure!(last["unbound"] == json!([]), "{last}");
    Ok(())
}
