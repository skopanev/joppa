//! requirement:jjhid61n over a real server: joppa_attach and the attach op store check_result
//! and valid_until on a file of a criterion, and refuse a bad value without a stored file.
#[path = "common/http_command.rs"]
mod http_command;
#[path = "common/manual_check.rs"]
mod manual_check;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use manual_check::{attach, files};
use record_world::fake_spaces;
use serde_json::{Value, json};

/// The file `id` in a list of files.
fn file<'a>(files: &'a [Value], id: &str) -> Result<&'a Value> {
    files
        .iter()
        .find(|f| f["id"] == id)
        .with_context(|| format!("no file {id}: {files:?}"))
}

// ac:kd844dik
// test:pew7836h
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn attach_stores_check_result_and_valid_until() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let fields = json!({"check_result":"pass","valid_until":"2026-12-31"});
    let first = manual_check::check(&w, "ac:a", fields).await?;
    let first = &first["file"];
    let id = first["id"].as_str().context("file id")?.to_owned();
    ensure!(
        first["check_result"] == "pass" && first["valid_until"] == "2026-12-31",
        "the joppa_attach reply: {first}"
    );
    let op = json!({"op":"attach","target":"ac:a","id":"f2","name":"b.txt","content_type":"text/plain",
        "size":first["size"],"hash":first["hash"],"check_result":"fail"});
    // The attach op goes through POST /api/workspaces/W/commands. MCP hides it.
    http_command::ok(&w, &w.bot, op.clone()).await?;
    let mut again = op;
    again["id"] = json!("f3");
    let refused = w.command(&w.bot, again).await?;
    ensure!(
        refused == Err("unknown operation".into()),
        "joppa_command attach: {refused:?}"
    );
    let listed = files(&w, "ac:a").await?;
    // The card counts the files and gives no file list.
    let object = w.object("ac:a").await?;
    ensure!(
        object["files_count"] == 2 && object.get("files").is_none(),
        "the joppa_objects card: {object}"
    );
    // joppa_attachments keeps the full file facts.
    ensure!(listed.len() == 2, "joppa_attachments: {listed:?}");
    let (one, two) = (file(&listed, &id)?, file(&listed, "f2")?);
    ensure!(
        one["check_result"] == "pass" && one["valid_until"] == "2026-12-31",
        "the first file: {one}"
    );
    ensure!(
        two["check_result"] == "fail" && two.get("valid_until").is_none(),
        "the second file: {two}"
    );
    ensure!(
        one["actor"] == "bot" && two["actor"] == "bot",
        "the actors: {one} {two}"
    );
    Ok(())
}

// ac:h506eqs2
// test:9cpsf58w
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn attach_refuses_a_bad_check_result_or_valid_until_and_stores_nothing() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let fake = fake_spaces::start();
    let keys = fake.keys();
    let mut calls = vec![(
        "ac:a",
        json!({"check_result":"ok"}),
        "check_result is pass or fail",
    )];
    for until in ["2026-13-01", "2026-1-05", "tomorrow"] {
        calls.push((
            "ac:a",
            json!({"check_result":"pass","valid_until":until}),
            "valid_until is a date YYYY-MM-DD",
        ));
    }
    calls.push((
        "ac:a",
        json!({"valid_until":"2026-12-31"}),
        "valid_until needs check_result",
    ));
    calls.push((
        "requirement:r",
        json!({"check_result":"pass"}),
        "a manual check belongs to a criterion",
    ));
    for (target, fields, refusal) in calls {
        let reply = attach(&w, &w.bot, target, fields.clone()).await?;
        ensure!(
            reply.as_ref().err().is_some_and(|e| e.contains(refusal)),
            "{target} {fields}: must be refused with {refusal:?}: {reply:?}"
        );
    }
    for target in ["ac:a", "requirement:r"] {
        let list = files(&w, target).await?;
        ensure!(list.is_empty(), "{target} has files: {list:?}");
    }
    ensure!(
        fake.keys() == keys,
        "Spaces has new keys: before {keys:?}, after {:?}",
        fake.keys()
    );
    Ok(())
}
