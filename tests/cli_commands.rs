//! requirement:cli-commands and requirement:launch-gate, the administrative binary: its exact
//! command list, the database commands as the given actor, and the launch gate on the commit
//! of an acceptance.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::process::{Command, Output};

const USAGE: &str = "commands: check-journal | migrate | backfill-board | serve [PORT HOST] | command WORKSPACE REQUEST_ID | read WORKSPACE | gate WORKSPACE COMMIT | list";

/// Run the binary with these arguments, the database URL and, when given, the actor.
fn joppa(url: &str, actor: Option<&str>, args: &[&str]) -> Result<Output> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_joppa"));
    cmd.args(args).env("JOPPA_DATABASE_URL", url);
    match actor {
        Some(actor) => cmd.env("JOPPA_ACTOR", actor),
        None => cmd.env_remove("JOPPA_ACTOR"),
    };
    Ok(cmd.output()?)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Run and require success. Gives the standard output.
fn ok(url: &str, args: &[&str]) -> Result<String> {
    let out = joppa(url, Some("owner"), args)?;
    ensure!(out.status.success(), "{args:?}: {}", text(&out.stderr));
    Ok(text(&out.stdout))
}

/// Run and require a failure. Gives the error text.
fn refused(url: &str, args: &[&str]) -> Result<String> {
    let out = joppa(url, Some("owner"), args)?;
    ensure!(
        !out.status.success(),
        "{args:?} passed: {}",
        text(&out.stdout)
    );
    Ok(text(&out.stderr))
}

// cli-commands-list
#[test]
fn the_cli_names_exactly_its_8_commands() -> Result<()> {
    // Nothing listens on port 9, so the usage needs no database.
    let out = joppa("postgresql://nobody@127.0.0.1:9/none", None, &[])?;
    ensure!(!out.status.success(), "no arguments passed");
    let stderr = text(&out.stderr);
    ensure!(
        stderr
            .lines()
            .any(|line| line.trim_start_matches("Error: ") == USAGE),
        "{stderr}"
    );
    Ok(())
}

// cli-commands-actor
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn check_journal_migrate_backfill_list_and_read_run_as_the_given_actor() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    ensure!(
        url.rsplit('/')
            .next()
            .is_some_and(|db| db.starts_with("joppa_test")),
        "test database required"
    );
    ensure!(
        ok(&url, &["migrate"])?.contains("journal schema ready"),
        "migrate"
    );
    ensure!(
        ok(&url, &["check-journal"])?.contains("journal is compatible"),
        "check-journal"
    );
    let pool = store::pool(&url)?;
    let ws = format!("cli-{}", uuid::Uuid::new_v4().simple());
    let title = format!("CLI {}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":title}),
    )
    .await?;
    ensure!(
        ok(&url, &["backfill-board"])?.contains("board projections created:"),
        "backfill-board"
    );
    let list: Value = serde_json::from_str(&ok(&url, &["list"])?)?;
    ensure!(
        list.as_array()
            .is_some_and(|items| items.iter().any(|i| i["title"] == json!(title))),
        "{list}"
    );
    let read: Value = serde_json::from_str(&ok(&url, &["read", &ws])?)?;
    ensure!(
        read.to_string().contains(&title),
        "read does not show the workspace"
    );
    let out = joppa(&url, None, &["read", &ws])?;
    ensure!(
        !out.status.success() && text(&out.stderr).contains("set JOPPA_ACTOR"),
        "read without an actor: {}",
        text(&out.stderr)
    );
    Ok(())
}

/// The current revision of an object, read through joppa_objects.
async fn revision(w: &record_world::World, address: &str) -> Result<u64> {
    let read = w.object(address).await?;
    read["revision"]
        .as_u64()
        .with_context(|| format!("no revision: {read}"))
}

async fn command(w: &record_world::World, command: Value) -> Result<Value> {
    w.command(&w.owner, command.clone())
        .await?
        .map_err(|e| anyhow::anyhow!("{command}: {e}"))
}

// launch-gate-commit
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_gate_passes_only_on_the_accepted_commit() -> Result<()> {
    let test = "tests/x.rs > t";
    let w = record_world::world(&[test]).await?;
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let tree = w
        .mcp(&w.owner, "joppa_tree", json!({}))
        .await?
        .map_err(|e| anyhow::anyhow!("joppa_tree: {e}"))?;
    command(&w, json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Make criteria launch conditions",
        "put_types":{"ac":{"title":"Criteria","role":"ac","status":"tests","root":false,"parents":["requirement"],"launch_blocker":true}}})).await?;
    let ac = revision(&w, record_world::A).await?;
    command(&w, json!({"op":"save_object","kind":"ac","id":"a","revision":ac,"parent":"requirement:r",
        "data":{"title":"A","owner":"owner","before_launch":true,"tests":[test]},"reason":"Require A before launch"})).await?;
    let ws = w.ws.as_str();
    let usage = refused(&url, &["gate", ws])?;
    ensure!(usage.contains("gate WORKSPACE COMMIT"), "{usage}");
    let before = refused(&url, &["gate", ws, "c1c1c1c"])?;
    ensure!(before.contains("has no acceptance on a commit"), "{before}");
    w.recorded("c1c1c1c", json!([{"test":test,"status":"pass"}]))
        .await?;
    let requirement = revision(&w, "requirement:r").await?;
    command(&w, json!({"op":"accept_commit","target":"requirement:r","revision":requirement,"commit":"c1c1c1c"})).await?;
    let passed = ok(&url, &["gate", ws, "c1c1c1c"])?;
    ensure!(
        passed.contains("1 launch conditions accepted on commit c1c1c1c"),
        "{passed}"
    );
    let other = refused(&url, &["gate", ws, "c2c2c2c"])?;
    ensure!(
        other.contains("accepted on commit c1c1c1c, not c2c2c2c"),
        "{other}"
    );
    Ok(())
}
