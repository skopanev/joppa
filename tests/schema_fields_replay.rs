//! requirement:generic-nodes: an older set_tree_schema with process_fields and
//! verification_subject still replays, while a live write refuses it.
mod common;

use common::*;
use joppa::commands::{self, Command};
use joppa::objects;
use joppa::removed_ops::SCHEMA_FIELDS;
use serde_json::json;

// generic-nodes-ac6
#[test]
fn an_older_schema_with_the_removed_fields_replays_and_a_live_write_refuses_it() {
    let mut w = World::new();
    for op in ["migrate_notes", "migrate_tasks", "migrate_schema"] {
        let cmd: Command = serde_json::from_value(json!({ "op": op })).unwrap();
        commands::apply_replay(&mut w.state, "owner", &cmd, w.position + 1, "now").unwrap();
        w.position += 1;
    }
    let schema = objects::schema(&w.state);
    let mut types = serde_json::to_value(&schema.types).unwrap();
    types["ac"]["process_fields"] = json!(["confirmed"]);
    types["task"]["references"]["projects"]["verification_subject"] = json!(true);
    let older = json!({"op":"set_tree_schema","revision":schema.revision,"types":types,
        "reason":"An older schema"});
    let live = w.apply("owner", older.clone()).unwrap_err().to_string();
    assert_eq!(live, SCHEMA_FIELDS);
    let cmd: Command = serde_json::from_value(older).unwrap();
    commands::apply_replay(&mut w.state, "owner", &cmd, w.position + 1, "now").unwrap();
    let replayed = objects::schema(&w.state);
    assert_eq!(replayed.revision, schema.revision + 1);
    assert_eq!(
        replayed.types["task"].references["projects"].verification_subject,
        Some(true),
        "the replay keeps the subject of the older Task saves"
    );
    // No read shows the removed fields.
    let shown = serde_json::to_string(&replayed.types).unwrap();
    assert!(
        !shown.contains("process_fields") && !shown.contains("verification_subject"),
        "{shown}"
    );
}
