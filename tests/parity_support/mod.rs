//! Shared helpers of the parity test: commands, reports, markers and the read dump.
// Each test target that includes this module uses a different part of it.
#![allow(dead_code, unused_imports)]
use anyhow::Result;
use joppa::store;
use serde_json::{Value, json};
use std::io::Write;

#[path = "fixture.rs"]
mod fixture;
pub use fixture::{ADDRESSES, CRITERIA, fixture, list};

/// The workspace of this test target. Each target is its own process and sets its own name,
/// so two targets can share one test database.
static WORKSPACE: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

pub fn use_workspace(name: &'static str) {
    WORKSPACE.set(name).expect("the workspace is set once");
}

pub fn ws() -> &'static str {
    WORKSPACE.get().copied().expect("call use_workspace first")
}
pub const FILE: &str = "tests/parity.rs";

pub async fn run(pool: &deadpool_postgres::Pool, n: &mut usize, command: Value) -> Result<()> {
    *n += 1;
    store::execute_input(pool, ws(), "owner", &format!("parity-{n}"), &command).await?;
    Ok(())
}

pub async fn revision(pool: &deadpool_postgres::Pool, address: &str) -> Result<Value> {
    let scope = store::ObjectScope {
        address: Some(address),
        ..Default::default()
    };
    Ok(store::read_objects(pool, ws(), "owner", &scope).await?["current_revision"].clone())
}

pub fn save(kind: &str, id: &str, parent: Option<&str>, data: Value) -> Value {
    json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"reason":"Build the fixture"})
}

/// Record the outcomes of `cases` for `commit` with joppa_record_results.
pub async fn record(
    pool: &deadpool_postgres::Pool,
    commit: &str,
    cases: &[(&str, &str)],
) -> Result<()> {
    let results: Vec<Value> = cases
        .iter()
        .map(|(name, result)| {
            let status = if *result == "skipped" { "skip" } else { result };
            json!({"test": format!("{FILE} > {name}"), "status": status})
        })
        .collect();
    store::record_results(
        pool,
        ws(),
        "owner",
        &json!({"commit":commit,"results":results}),
    )
    .await?;
    Ok(())
}

/// Replace times and item IDs, which differ between runs, with markers.
pub fn normal(value: &mut Value) {
    match value {
        Value::String(s) => {
            let b = s.as_bytes();
            if b.len() >= 19
                && b[4] == b'-'
                && b[7] == b'-'
                && (b[10] == b' ' || b[10] == b'T')
                && b[13] == b':'
            {
                *s = "<time>".into();
            } else if s.starts_with("parity-")
                && s.len() > 7
                && s[7..]
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
            {
                *s = "<item-id>".into();
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normal),
        Value::Object(map) => {
            for (key, inner) in map.iter_mut() {
                if key == "item_id" || key == "system_id" {
                    *inner = json!("<item-id>");
                } else {
                    normal(inner);
                }
            }
        }
        _ => {}
    }
}

pub async fn dump(
    pool: &deadpool_postgres::Pool,
    out: &mut std::fs::File,
    label: &str,
    addresses: &[&str],
    criteria: &[&str],
) -> Result<()> {
    let mut line = |key: String, mut value: Value| -> Result<()> {
        normal(&mut value);
        writeln!(out, "{}", json!({"label":label,"read":key,"value":value}))?;
        Ok(())
    };
    let mut cursor: Option<String> = None;
    loop {
        let scope = store::ObjectScope {
            parent: Some("*"),
            limit: 100,
            cursor: cursor.as_deref(),
            ..Default::default()
        };
        let page = store::read_objects(pool, ws(), "owner", &scope).await?;
        cursor = page["next_cursor"].as_str().map(String::from);
        line("list *".into(), page)?;
        if cursor.is_none() {
            break;
        }
    }
    for (name, scope) in [
        (
            "list domain",
            store::ObjectScope {
                kind: Some("domain"),
                limit: 50,
                ..Default::default()
            },
        ),
        (
            "list ac:a1",
            store::ObjectScope {
                parent: Some("ac:a1"),
                ..Default::default()
            },
        ),
        (
            "list unstarted",
            store::ObjectScope {
                work_filter: Some("unstarted"),
                ..Default::default()
            },
        ),
    ] {
        line(
            name.into(),
            store::read_objects(pool, ws(), "owner", &scope).await?,
        )?;
    }
    for address in addresses {
        let scope = store::ObjectScope {
            address: Some(address),
            ..Default::default()
        };
        line(
            format!("object {address}"),
            store::read_objects(pool, ws(), "owner", &scope).await?,
        )?;
    }
    for criterion in criteria {
        line(
            format!("tests {criterion}"),
            store::criterion_tests(pool, ws(), "owner", criterion).await?,
        )?;
    }
    line(
        "tasks all".into(),
        store::task_queue(pool, ws(), "owner", "all", 100, None).await?,
    )?;
    Ok(())
}
