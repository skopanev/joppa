//! migrate_purpose removes the purpose of each Task once. Its revisions keep the contract
//! revision, a purpose of more than one word joins the Task text, and later saves have no
//! purpose. The Task types lose the decision rule parent_decision.
mod common;
use common::*;
use joppa::objects;
use serde_json::{Value, json};

/// Apply an internal migration event, as the journal replay does.
fn internal(w: &mut World, op: &str) {
    let command: joppa::commands::Command = serde_json::from_value(json!({ "op": op })).unwrap();
    joppa::commands::apply_replay(
        &mut w.state,
        "owner",
        &command,
        w.position + 1,
        "2026-01-01T00:00:00Z",
    )
    .unwrap_or_else(|e| panic!("{op}: {e}"));
    w.position += 1;
}

fn data(w: &World, key: &str) -> Value {
    w.state.objects[key].current().data.clone()
}

fn save(w: &mut World, key: &str, data: Value) -> anyhow::Result<()> {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.apply(
        "owner",
        json!({"op":"save_object","kind":kind,"id":id,"revision":revision,
        "parent":"ac:ios","data":data,"reason":"Write the Task"}),
    )
}

fn task(body: &str, purpose: &str) -> Value {
    json!({"title":"Implement access","owner":"owner","body":body,"purpose":purpose,"projects":[]})
}

/// A workspace after every earlier migration, with three Tasks that have a purpose.
fn before() -> World {
    let mut w = World::new();
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
        "migrate_consumers",
        "migrate_consumer_drops",
        "migrate_test_rules",
        "migrate_note_parents",
        "migrate_body",
    ] {
        internal(&mut w, op);
    }
    assert!(!w.state.purpose_migrated);
    save(
        &mut w,
        "task:label",
        task("Keep the paid access", "implementation"),
    )
    .unwrap();
    save(
        &mut w,
        "task:text",
        task("Keep the paid access", "Prove the restart rule."),
    )
    .unwrap();
    let repeated = task(
        "Keep the paid access. Prove the restart rule.",
        "Prove the restart rule.",
    );
    save(&mut w, "task:same", repeated).unwrap();
    w
}

#[test]
fn the_migration_removes_each_purpose_and_keeps_the_contract_revision() {
    let mut w = before();
    let keys = ["task:label", "task:text", "task:same"];
    let contract = |w: &World, key: &str| objects::contract_revision(&w.state.objects[key]);
    let pins: Vec<_> = keys.iter().map(|k| contract(&w, k)).collect();
    let revisions: Vec<_> = keys
        .iter()
        .map(|k| w.state.objects[*k].current().revision)
        .collect();
    let decision = |w: &World| {
        objects::schema(&w.state)
            .types
            .values()
            .any(|t| t.parent_decision)
    };
    assert!(decision(&w));
    internal(&mut w, "migrate_purpose");
    assert!(!decision(&w));
    assert_eq!(
        objects::schema(&w.state).reason,
        joppa::purpose_migration::REASON
    );
    for (i, key) in keys.iter().enumerate() {
        assert!(data(&w, key).get("purpose").is_none(), "{key}");
        assert_eq!(contract(&w, key), pins[i], "{key}");
        assert_eq!(w.state.objects[*key].current().revision, revisions[i] + 1);
        assert_eq!(w.state.task_process[*key].purpose, "");
    }
    assert_eq!(data(&w, "task:label")["body"], "Keep the paid access");
    let joined = "Keep the paid access\n\nProve the restart rule.";
    assert_eq!(data(&w, "task:text")["body"], joined);
    assert_eq!(w.state.task_process["task:text"].contract, joined);
    let same = "Keep the paid access. Prove the restart rule.";
    assert_eq!(data(&w, "task:same")["body"], same);
    let process = joppa::tasks::process(&w.state, "task:label").unwrap();
    assert!(process.get("purpose").is_none(), "{process}");
    // The event runs once.
    let once = serde_json::to_value(&w.state).unwrap();
    internal(&mut w, "migrate_purpose");
    assert_eq!(serde_json::to_value(&w.state).unwrap(), once);
}

#[test]
fn after_the_migration_a_task_save_and_a_task_type_have_no_purpose() {
    let mut w = before();
    internal(&mut w, "migrate_purpose");
    let mut label = data(&w, "task:label");
    label["purpose"] = json!("implementation");
    let error = save(&mut w, "task:label", label).unwrap_err();
    assert_eq!(error.to_string(), joppa::purpose_migration::REFUSAL);
    let mut edit = data(&w, "task:label");
    edit["title"] = json!("Implement the paid access");
    save(&mut w, "task:label", edit).unwrap();
    save(&mut w, "task:new", task("Keep it", "x")).unwrap_err();
    let mut fresh = task("Keep it", "x");
    fresh.as_object_mut().unwrap().remove("purpose");
    save(&mut w, "task:new", fresh).unwrap();
    let s = objects::schema(&w.state);
    let mut types = serde_json::to_value(&s.types).unwrap();
    types["task"]["required_fields"] = json!({"purpose":{"contains":["a"],"error":"Purpose"}});
    let error = w
        .apply(
            "owner",
            json!({"op":"set_tree_schema","revision":s.revision,"types":types,
            "reason":"Require a purpose"}),
        )
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("task: {}", joppa::purpose_migration::REFUSAL)
    );
    let mut types = serde_json::to_value(&s.types).unwrap();
    types["task"]["parent_decision"] = json!(true);
    let error = w
        .apply(
            "owner",
            json!({"op":"set_tree_schema","revision":s.revision,"types":types,
            "reason":"Require a decision"}),
        )
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("task: {}", joppa::purpose_migration::DECISION_REFUSAL)
    );
}
