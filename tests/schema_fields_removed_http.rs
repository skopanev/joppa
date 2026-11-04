//! requirement:generic-nodes over a real server: a schema type has no process_fields, and a
//! reference has no verification_subject.
#[path = "common/generic_world.rs"]
mod generic_world;
use anyhow::{Result, ensure};
use generic_world::{World, world};
use joppa::removed_ops::SCHEMA_FIELDS;
use serde_json::{Value, json};

/// The current schema that joppa_tree gives.
async fn tree(w: &World) -> Result<Value> {
    let tree = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    Ok(tree.map_err(anyhow::Error::msg)?["schema"].clone())
}

/// A set_tree_schema that replaces the type task with `task`.
fn put_task(revision: &Value, task: Value) -> Value {
    json!({"op":"set_tree_schema","revision":revision,"put_types":{"task":task},
        "reason":"Let a Task cover criteria"})
}

// generic-nodes-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_schema_with_process_fields_or_verification_subject_is_refused() -> Result<()> {
    let w = world(&[]).await?;
    let before = tree(&w).await?;
    let covers = json!({"target":"ac","title":"Covers","multiple":true,"required":false});
    let task = json!({"title":"Tasks","role":"task","status":"none","parents":["ac","requirement"],
        "references":{"covers":covers}});
    let mut fields = task.clone();
    fields["process_fields"] = json!(["confirmed"]);
    let mut subject = task.clone();
    subject["references"]["covers"]["verification_subject"] = json!(false);
    for refused in [fields, subject] {
        let reply = w
            .command(&w.owner, put_task(&before["revision"], refused))
            .await?;
        ensure!(reply == Err(SCHEMA_FIELDS.into()), "{reply:?}");
    }
    ensure!(
        tree(&w).await?["revision"] == before["revision"],
        "no schema revision"
    );
    w.command(&w.owner, put_task(&before["revision"], task))
        .await?
        .map_err(anyhow::Error::msg)?;
    let after = tree(&w).await?;
    ensure!(
        after["types"]["task"]["references"]["covers"] == covers,
        "{after}"
    );
    let text = after["types"].to_string();
    ensure!(
        !text.contains("process_fields") && !text.contains("verification_subject"),
        "joppa_tree shows neither field: {text}"
    );
    Ok(())
}
