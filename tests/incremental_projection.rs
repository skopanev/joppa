//! The incremental projection must agree with a full rebuild, row for row.
//!
//! Reading the dirty closure cannot prove that it is complete: a missing edge
//! looks correct until the data disagrees. So this test runs a command
//! sequence, and after every single command it compares the projection tables
//! written incrementally against the same tables rebuilt from current state.
//!
//! Two rebuilds run at each step, because they fail for different reasons.
//! Dropping only board_cache catches an update the closure never made.
//! Dropping the workspace's projection rows as well catches a row the closure
//! should have removed and left behind.
mod common;
use common::*;
use joppa::store;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Derived tables that a rebuild must reproduce exactly.
const TABLES: [&str; 8] = [
    "objects",
    "object_revisions",
    "object_links",
    "ac_owners",
    "state_entries",
    "state_revisions",
    "task_queue",
    "search_documents",
];

/// Columns that differ between an incremental write and a rebuild by contract.
/// Everything else must match, so this list stays short and explicit.
fn dropped(table: &str) -> &'static [&'static str] {
    match table {
        // terms is generated from title and content, so comparing those is
        // enough. embedding is filled asynchronously and may be absent.
        "search_documents" => &["terms", "embedding"],
        _ => &[],
    }
}

type Snapshot = BTreeMap<String, Vec<String>>;

async fn snapshot(pool: &deadpool_postgres::Pool, workspace: &str) -> anyhow::Result<Snapshot> {
    let conn = pool.get().await?;
    let mut out = Snapshot::new();
    for table in TABLES {
        let statement = format!("SELECT to_jsonb(t)::text FROM joppa.{table} t WHERE workspace=$1");
        let mut rows = vec![];
        for row in conn.query(&statement, &[&workspace]).await? {
            let mut value: Value = serde_json::from_str(row.get(0))?;
            if let Some(object) = value.as_object_mut() {
                for key in dropped(table) {
                    object.remove(*key);
                }
                // The status moment is kept from the previous row while tone
                // and label hold. A rebuild has no previous row, so only the
                // tone and the label are comparable here.
                if let Some(signal) = object.get_mut("signal").and_then(Value::as_object_mut) {
                    signal.remove("at");
                }
            }
            rows.push(value.to_string());
        }
        // Order by content: the tables have different keys and a stable row
        // order is all the comparison needs.
        rows.sort();
        out.insert(table.into(), rows);
    }
    Ok(out)
}

async fn rebuild(
    pool: &deadpool_postgres::Pool,
    workspace: &str,
    wipe: bool,
) -> anyhow::Result<Snapshot> {
    {
        let conn = pool.get().await?;
        if wipe {
            for table in TABLES {
                let statement = format!("DELETE FROM joppa.{table} WHERE workspace=$1");
                conn.execute(&statement, &[&workspace]).await?;
            }
        }
        conn.execute(
            "DELETE FROM joppa.board_cache WHERE workspace=$1",
            &[&workspace],
        )
        .await?;
    }
    store::backfill_board(pool).await?;
    snapshot(pool, workspace).await
}

async fn step(
    pool: &deadpool_postgres::Pool,
    workspace: &str,
    counter: &mut usize,
    label: &str,
    command: Value,
) -> anyhow::Result<()> {
    *counter += 1;
    store::execute_input(pool, workspace, "owner", &format!("op-{counter}"), &command)
        .await
        .map_err(|e| anyhow::anyhow!("{label}: {command}: {e:#}"))?;
    let incremental = snapshot(pool, workspace).await?;
    for wipe in [false, true] {
        let rebuilt = rebuild(pool, workspace, wipe).await?;
        for table in TABLES {
            anyhow::ensure!(
                incremental[table] == rebuilt[table],
                "after {label} the incremental joppa.{table} differs from a rebuild with wipe={wipe}\nincremental: {:#?}\nrebuilt: {:#?}",
                incremental[table],
                rebuilt[table]
            );
        }
    }
    Ok(())
}

/// One sequence, ordered by the seed. The seed only permutes steps that do not
/// depend on each other, so every seed must reach the same final projection.
fn sequence(seed: usize) -> Vec<(&'static str, Value)> {
    let scope = "One agreed build set";
    let subject =
        json!({"app":"sha-a","backend":"sha-b","infra":"config-v1","environment":"staging"});
    let note = |id: &str, title: &str, parent: Option<&str>, links: Value| {
        json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":parent,
            "data":{"title":title,"owner":"owner","body":"Recorded input"},"links":links,
            "reason":"Record an input"})
    };
    let mut steps: Vec<(&'static str, Value)> = vec![
        ("confirm", json!({"op":"confirm","req":"r","revision":1})),
        ("assess ios", assess("ios", true)),
        ("assess key", assess("key", true)),
    ];
    // Three nested inputs: a change at the top must reach the bottom.
    let nesting = vec![
        ("note top", note("top", "Top input", None, json!([]))),
        (
            "note middle",
            note("middle", "Middle input", Some("note:top"), json!([])),
        ),
        (
            "note leaf",
            note("leaf", "Leaf input", Some("note:middle"), json!([])),
        ),
    ];
    // Two inputs that point at each other: an ordinary cycle is allowed and
    // must not make the closure loop.
    let cycle = vec![
        ("note left", note("left", "Left input", None, json!([]))),
        (
            "note right",
            note(
                "right",
                "Right input",
                None,
                json!([{"target":"note:left","relation":"related"}]),
            ),
        ),
        (
            "note left points back",
            json!({"op":"save_object","kind":"note","id":"left","revision":1,"parent":null,
                "data":{"title":"Left input","owner":"owner","body":"Recorded input"},
                "links":[{"target":"note:right","relation":"related"}],"reason":"Close the cycle"}),
        ),
    ];
    if seed.is_multiple_of(2) {
        steps.extend(nesting);
        steps.extend(cycle);
    } else {
        steps.extend(cycle);
        steps.extend(nesting);
    }
    steps.extend(vec![
        (
            "empty group",
            json!({"op":"domain","id":"unused","title":"Unused","owner":"owner"}),
        ),
        (
            "archive group",
            json!({"op":"archive_object","target":"domain:unused","revision":1,"reason":"Archive the empty group"}),
        ),
        (
            "write while archived",
            json!({"op":"note","target":"domain:d","id":"archive-context","text":"Keep the group archived"}),
        ),
        (
            "restore group",
            json!({"op":"restore_object","target":"domain:unused","revision":2,"reason":"Restore the group"}),
        ),
        (
            "edit restored group",
            json!({"op":"edit_domain","id":"unused","title":"Restored","owner":"owner","reason":"Edit the restored group"}),
        ),
        (
            "verification scope",
            json!({"op":"define_verification_scope","req":"r","revision":1,
                "subject_keys":["app","backend","infra","environment"],"scope":scope,
                "reason":"Agree the components"}),
        ),
        (
            "plan",
            json!({"op":"plan","req":"r","revision":1,"run":"r1","subject":subject,"scope":scope,
                "discovery_complete":true,
                "checks":[{"id":"t-ios[ios]","ac":"ios","method":"automated","expected":["access survives"]},
                          {"id":"t-key[key]","ac":"key","method":"automated","expected":["read and verify only"]}]}),
        ),
        // Evidence only: no document changes at all, yet readiness moves.
        (
            "result ios",
            json!({"op":"result","req":"r","revision":1,"run":"r1",
                "evidence":{"check":"t-ios[ios]","outcome":"pass","observed":["access survives"],
                "note":"Recorded by the suite"}}),
        ),
        (
            "result key",
            json!({"op":"result","req":"r","revision":1,"run":"r1",
                "evidence":{"check":"t-key[key]","outcome":"pass","observed":["read and verify only"],
                "note":"Recorded by the suite"}}),
        ),
        ("finish", json!({"op":"finish","req":"r","revision":1,"run":"r1"})),
        // A blocking dependency on a criterion: its evidence decides readiness.
        (
            "note waits for a criterion",
            json!({"op":"save_object","kind":"note","id":"waiting","revision":0,"parent":null,
                "data":{"title":"Waiting input","owner":"owner","body":"Needs the criterion"},
                "links":[{"target":"ac:key","relation":"depends_on","revision":1,"blocking":true}],
                "reason":"Depend on a criterion"}),
        ),
        (
            "task",
            json!({"op":"task","req":"r","revision":1,"id":"t1","title":"Carry the work",
                "acs":["ios"],"projects":["app"],"purpose":"implementation",
                "contract":"Change the access path"}),
        ),
        (
            "claim",
            json!({"op":"claim_task","req":"r","revision":1,"id":"t1"}),
        ),
        (
            "process the input",
            json!({"op":"set_note_status","target":"note:top","revision":1,"status":"processed",
                "reason":"Applied to the tree"}),
        ),
        // A new revision resets readiness and marks carried work for review.
        ("revise", revision(1)),
    ]);
    steps
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn incremental_projection_matches_a_full_rebuild() -> anyhow::Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let conn = pool.get().await?;
    let name: String = conn
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    anyhow::ensure!(
        name.starts_with("joppa_test"),
        "isolated test database required"
    );
    drop(conn);
    store::migrate(&pool).await?;
    for seed in [0usize, 1] {
        let ws = format!(
            "incremental-{seed}-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        let mut counter = 0;
        for command in setup() {
            step(&pool, &ws, &mut counter, "setup", command).await?;
        }
        for (label, command) in sequence(seed) {
            step(&pool, &ws, &mut counter, label, command).await?;
        }
    }
    Ok(())
}
