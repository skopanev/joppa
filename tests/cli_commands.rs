//! requirement:cli-commands and requirement:launch-gate, the administrative binary: its exact
//! command list, the database commands as the given actor, and the refusal of the removed
//! acceptance.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Output, Stdio};

const USAGE: &str = "commands: check-journal | migrate | backfill-board | serve [PORT HOST] | command WORKSPACE REQUEST_ID | read WORKSPACE | list";

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

// ac:cli-command-list
#[test]
fn the_cli_names_exactly_its_7_commands() -> Result<()> {
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

/// Run `command WORKSPACE REQUEST_ID` as the owner with `input` on the standard input.
fn command(url: &str, ws: &str, input: &Value) -> Result<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_joppa"))
        .args(["command", ws, &uuid::Uuid::new_v4().to_string()])
        .env("JOPPA_DATABASE_URL", url)
        .env("JOPPA_ACTOR", "owner")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .context("stdin")?
        .write_all(input.to_string().as_bytes())?;
    Ok(child.wait_with_output()?)
}

// ac:acceptance-needs-green
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_cli_refuses_an_acceptance_with_the_replacement() -> Result<()> {
    let test = "tests/x.rs > t";
    let w = record_world::world(&[test]).await?;
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    w.recorded("c1c1c1c", json!([{"test":test,"status":"pass"}]))
        .await?;
    let read = w.object("requirement:r").await?;
    let revision = read["revision"].as_u64().context("revision")?;
    let ws = w.ws.as_str();
    for input in [
        json!({"op":"accept_commit","target":"requirement:r","revision":revision,"commit":"c1c1c1c"}),
        json!({"op":"accept","req":"r","revision":revision,"run":"run-1"}),
    ] {
        let out = command(&url, ws, &input)?;
        let error = text(&out.stderr);
        ensure!(
            !out.status.success() && error.contains("manual acceptance is gone"),
            "{input}: {error}"
        );
    }
    ensure!(
        w.object("requirement:r").await?["process"]
            .get("acceptance")
            .is_none(),
        "the CLI accepted"
    );
    // The CLI has no gate command.
    let gate = refused(&url, &["gate", ws, "c1c1c1c"])?;
    ensure!(gate.contains(USAGE), "{gate}");
    Ok(())
}
