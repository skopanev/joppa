//! A write to a large workspace: the time and the work of single live writes.
use anyhow::{Result, ensure};
use joppa::{probe, store};
use serde_json::{Value, json};
use std::time::Instant;

type Pool = deadpool_postgres::Pool;

async fn batch(pool: &Pool, ws: &str, commands: Vec<Value>) -> Result<()> {
    for chunk in commands.chunks(100) {
        let operations: Vec<store::BatchOperation> = chunk
            .iter()
            .map(|command| store::BatchOperation {
                reference: None,
                command: command.clone(),
            })
            .collect();
        let id = uuid::Uuid::new_v4().to_string();
        store::execute_remote_batch(pool, ws, "owner", &id, &operations).await?;
    }
    Ok(())
}

fn node(kind: &str, id: &str, parent: Option<String>, relations: Value) -> Value {
    json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,
        "data":{"title":format!("{kind} {id}"),"owner":"owner","body":"The rule"},
        "relations":relations,"reason":"Create"})
}

/// A workspace of `domains` x 5 capabilities x 6 requirements x `acs` criteria, each with one
/// test node. Each tenth criterion depends on a criterion of the requirement before it.
async fn tree(pool: &Pool, domains: usize, acs: usize) -> Result<String> {
    let ws = format!("write-latency-{}", uuid::Uuid::new_v4().simple());
    let op = json!({"op":"workspace","title":"Write latency"});
    store::execute_input(pool, &ws, "owner", "workspace", &op).await?;
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    batch(pool, &ws, vec![json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "domain":{"title":"Domains","role":"group","status":"children","root":true,"parents":[]},
        "capability":{"title":"Capabilities","role":"group","status":"children","parents":["domain"]},
        "requirement":{"title":"Requirements","role":"requirement","status":"children","parents":["capability"]},
        "ac":{"title":"Criteria","role":"ac","status":"children","parents":["requirement"]},
        "test":{"title":"Tests","role":"test","status":"own","parents":["ac"],"title_preview_length":400}}})]).await?;
    let mut commands = vec![];
    let mut last_req: Option<String> = None;
    for d in 0..domains {
        commands.push(node("domain", &format!("d{d}"), None, json!({})));
        for c in 0..5 {
            let cap = format!("d{d}c{c}");
            commands.push(node(
                "capability",
                &cap,
                Some(format!("domain:d{d}")),
                json!({}),
            ));
            for r in 0..6 {
                let req = format!("{cap}r{r}");
                let parent = Some(format!("capability:{cap}"));
                commands.push(node("requirement", &req, parent, json!({})));
                for a in 0..acs {
                    let ac = format!("{req}a{a}");
                    let relations = match (&last_req, a % 10) {
                        (Some(prev), 0) => {
                            json!({"depends_on":{"edges":[{"to":format!("ac:{prev}a1")}]}})
                        }
                        (Some(prev), 5) => {
                            json!({"related":{"edges":[{"to":format!("ac:{prev}a2")}]}})
                        }
                        _ => json!({}),
                    };
                    commands.push(node(
                        "ac",
                        &ac,
                        Some(format!("requirement:{req}")),
                        relations,
                    ));
                    let mut test = node(
                        "test",
                        &format!("{ac}t"),
                        Some(format!("ac:{ac}")),
                        json!({}),
                    );
                    test["data"]["title"] = json!(format!("tests/big.rs > {ac}"));
                    commands.push(test);
                }
                last_req = Some(req);
            }
        }
    }
    batch(pool, &ws, commands).await?;
    Ok(ws)
}

async fn current(pool: &Pool, ws: &str, address: &str) -> Result<Value> {
    let scope = store::ObjectScope {
        address: Some(address),
        ..Default::default()
    };
    store::read_objects(pool, ws, "owner", &scope).await
}

use probe::COUNTERS;

/// One live write: its time in ms and the counts it adds to each probe.
async fn timed(pool: &Pool, ws: &str, command: Value) -> Result<(f64, Value)> {
    let before: Vec<u64> = COUNTERS.iter().map(|(_, c)| probe::read(c)).collect();
    let start = Instant::now();
    let id = uuid::Uuid::new_v4().to_string();
    store::execute_remote_input(pool, ws, "owner", &id, &command).await?;
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    let counts: serde_json::Map<String, Value> = COUNTERS
        .iter()
        .zip(before)
        .map(|((name, c), b)| (name.to_string(), json!(probe::read(c) - b)))
        .collect();
    Ok((ms, Value::Object(counts)))
}

/// The four measured writes on the criterion `ac` of `ws`: a save, a note, an archive of its
/// test node and an archive of the criterion after that.
async fn writes(pool: &Pool, ws: &str, ac: &str) -> Result<Value> {
    let object = current(pool, ws, &format!("ac:{ac}")).await?;
    let mut data = object["document"]["data"].clone();
    data["body"] = json!("The new rule");
    let save = json!({"op":"save_object","kind":"ac","id":ac,"revision":object["current_revision"],
        "parent":object["document"]["parent"],"data":data,"reason":"Clarify"});
    let mut out = json!({});
    out["save_object"] = json!(timed(pool, ws, save).await?);
    let note = json!({"op":"note","target":format!("ac:{ac}"),"id":"n1","text":"Seen"});
    out["note"] = json!(timed(pool, ws, note).await?);
    let test = current(pool, ws, &format!("test:{ac}t")).await?;
    let archive = |target: String, revision: &Value| json!({"op":"archive_object","target":target,"revision":revision,"reason":"Not needed"});
    let leaf = archive(format!("test:{ac}t"), &test["current_revision"]);
    out["archive_leaf"] = json!(timed(pool, ws, leaf).await?);
    let object = current(pool, ws, &format!("ac:{ac}")).await?;
    let parent = archive(format!("ac:{ac}"), &object["current_revision"]);
    out["archive_parent"] = json!(timed(pool, ws, parent).await?);
    Ok(out)
}

// ac:oj6w7c2s
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_write_on_a_large_workspace_serializes_only_the_rows_that_it_changes() -> Result<()> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    // 1 domain, 5 capabilities, 30 requirements, 60 criteria and 60 test nodes.
    let small = tree(&pool, 1, 2).await?;
    // 8 domains, 40 capabilities, 240 requirements, 2400 criteria and 2400 test nodes.
    let large = tree(&pool, 8, 10).await?;
    let small = writes(&pool, &small, "d0c0r1a1").await?;
    let large = writes(&pool, &large, "d3c2r1a3").await?;
    println!("{}", json!({"small":small,"large":large}));
    for write in ["save_object", "note", "archive_leaf", "archive_parent"] {
        let rows = |w: &Value| w[write][1]["rows"].as_u64().unwrap_or(u64::MAX);
        // The rows that a write serializes do not grow with the workspace.
        ensure!(rows(&large) == rows(&small), "{write}: {small} {large}");
        ensure!(
            rows(&large) <= 20,
            "{write} serialized {} rows",
            rows(&large)
        );
    }
    // A live save copies the workspace once, for its check of an unchanged save, and an
    // event that writes no document syncs no document.
    ensure!(large["save_object"][1]["save_copy"] == 1, "{large}");
    ensure!(large["note"][1]["sync"] == 0, "{large}");
    Ok(())
}
