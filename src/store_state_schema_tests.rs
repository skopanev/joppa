//! The schema rules that the stored projections read (store_state_schema.rs).
use super::*;
use serde_json::json;

fn workspace(revision: u32, types: Value, extra: Value) -> Workspace {
    let mut schema = json!({"revision": revision, "types": types, "actor": "bob",
        "at": "2026-11-27T10:00:00Z", "position": revision, "reason": "Edit"});
    for (key, value) in extra.as_object().into_iter().flatten() {
        schema[key] = value.clone();
    }
    let mut w = Workspace::default();
    w.tree_schema.push(serde_json::from_value(schema).unwrap());
    w
}

fn task(extra: Value) -> Value {
    let mut t = json!({"title": "Tasks", "role": "task", "status": "own", "root": true,
        "parents": []});
    for (key, value) in extra.as_object().into_iter().flatten() {
        t[key] = value.clone();
    }
    json!({ "task": t })
}

// ac:SPEEDSCHEMA
#[test]
fn a_schema_write_of_the_look_and_the_limits_keeps_the_projection() {
    let before = workspace(1, task(json!({})), json!({}));
    let look = json!({"title": "Work", "color": "#aa0000", "badge": "W", "title_size": 2,
        "field_max_length": 2048, "title_preview_length": 300, "question": "What?",
        "done": {"stage": "done"}});
    let after = workspace(2, task(look), json!({"priorities": ["high", "low"]}));
    assert!(projection_schema_same(&after, &before).unwrap());
}

// ac:SPEEDSCHEMA
#[test]
fn a_schema_write_of_a_projection_rule_projects_again() {
    let before = workspace(1, task(json!({})), json!({}));
    for change in [
        json!({"status": "children"}),
        json!({"role": "requirement"}),
        json!({"inbox": true}),
        json!({"reset_on_change": true}),
        json!({"fields": {"stage": {"type": "enum", "values": ["a", "b"]}}}),
    ] {
        let after = workspace(2, task(change.clone()), json!({}));
        assert!(
            !projection_schema_same(&after, &before).unwrap(),
            "{change}"
        );
    }
    let filters = workspace(2, task(json!({})), json!({"filters": ["stage"]}));
    assert!(!projection_schema_same(&filters, &before).unwrap());
}
