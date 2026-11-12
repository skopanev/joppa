//! The removed guards: a criterion may move to another requirement.
mod common;
use common::*;
use joppa::objects;
use serde_json::{Value, json};

fn internal(w: &mut World, op: &str) {
    let command: joppa::commands::Command = serde_json::from_value(json!({ "op": op })).unwrap();
    joppa::commands::apply_replay(&mut w.state, "owner", &command, w.position + 1, "now")
        .unwrap_or_else(|e| panic!("{op}: {e}"));
    w.position += 1;
}

/// A current workspace: every migration ran, and both criteria are present.
fn current() -> World {
    let mut w = World::new();
    w.confirm();
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
        "migrate_purpose",
        "migrate_generic_nodes",
        "migrate_decision_required",
    ] {
        internal(&mut w, op);
    }
    w
}

fn revision(w: &World, key: &str) -> u32 {
    w.state.objects.get(key).map_or(0, |o| o.current().revision)
}

fn save(w: &mut World, actor: &str, key: &str, parent: &str, data: Value) -> anyhow::Result<()> {
    let (kind, id) = key.split_once(':').unwrap();
    w.apply(
        actor,
        json!({"op":"save_object","kind":kind,"id":id,"revision":revision(w, key),
        "parent":parent,"data":data,"reason":"Change the work"}),
    )
}

fn task(title: &str) -> Value {
    json!({"title":title,"owner":"owner","body":"Implement the agreed part"})
}

#[test]
fn a_criterion_moves_to_another_requirement_and_leaves_the_old_one() {
    let mut w = current();
    let other = json!({"title":"Keys stay safe","owner":"owner","body":"Only the key rule"});
    save(&mut w, "owner", "requirement:s", "capability:c", other).unwrap();
    let ios = revision(&w, "ac:ios");
    let mut key = w.state.objects["ac:key"].current().data.clone();
    key["title"] = json!("Before production key has exactly read and verify permissions");
    save(&mut w, "owner", "ac:key", "requirement:s", key).unwrap();
    let moved = w.state.objects["ac:key"].current();
    assert_eq!(moved.parent.as_deref(), Some("requirement:s"));
    let (req, _, _) = objects::criterion_context(&w.state, "ac:key").unwrap();
    assert_eq!(req, "requirement:s");
    let old = w.state.requirements["r"].revisions.last().unwrap();
    assert!(old.acs.iter().all(|ac| ac.id != "key"), "{:?}", old.acs);
    assert!(old.acs.iter().any(|ac| ac.id == "ios"));
    assert_eq!(
        w.state.objects["ac:ios"].current().parent.as_deref(),
        Some("requirement:r")
    );
    assert_eq!(revision(&w, "ac:ios"), ios);
    // A later write leaves both criteria and both requirements as they are.
    let before: Vec<u32> = ["ac:key", "ac:ios", "requirement:r", "requirement:s"]
        .iter()
        .map(|k| revision(&w, k))
        .collect();
    save(&mut w, "owner", "task:t", "ac:key", task("T")).unwrap();
    let after: Vec<u32> = ["ac:key", "ac:ios", "requirement:r", "requirement:s"]
        .iter()
        .map(|k| revision(&w, k))
        .collect();
    assert_eq!(before, after);
}
