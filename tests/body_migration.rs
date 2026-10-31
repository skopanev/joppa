//! migrate_body moves the old text fields of every type into body once. Its revisions keep
//! the contract revision, the assessments and the test boundaries, and later writes name
//! the text body only.
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

fn save(w: &mut World, key: &str, parent: &str, data: Value) -> anyhow::Result<()> {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.apply(
        "owner",
        json!({"op":"save_object","kind":kind,"id":id,"revision":revision,
        "parent":parent,"data":data,"reason":"Write the text"}),
    )
}

fn assess_all(w: &mut World) {
    let number = w.state.requirements["r"].revisions.last().unwrap().number;
    for ac in ["ios", "key"] {
        w.ok(
            "agent",
            json!({"op":"assess","req":"r","revision":number,"ac":ac,
            "assessment":{"ready":true,"reason":"Known scope"}}),
        );
    }
}

const GWT: &str =
    "Given: a buyer.\nWhen: the app restarts.\nThen: access stays.\nEvidence: a test.";

/// A workspace after every earlier migration, with an AC description and a Task contract.
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
    ] {
        internal(&mut w, op);
    }
    assert!(!w.state.body_migrated);
    for key in ["ac:ios", "ac:key"] {
        let mut ac = data(&w, key);
        ac["description"] = json!(GWT);
        save(&mut w, key, "requirement:r", ac).unwrap();
    }
    let task = json!({"title":"Implement access","owner":"owner","contract":"Keep the paid access",
        "purpose":"implementation","projects":[]});
    save(&mut w, "task:t1", "ac:ios", task).unwrap();
    assess_all(&mut w);
    // A required description rule of the AC type becomes a required body rule.
    let s = objects::schema(&w.state);
    let mut types = serde_json::to_value(&s.types).unwrap();
    types["ac"]["required_fields"] = json!({"description":{"contains":["Given:"],"error":"Given"}});
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":s.revision,"types":types,
        "reason":"Require Given"}),
    );
    w
}

// body-text-ac1
#[test]
fn the_migration_moves_each_old_text_into_body_and_keeps_pins_assessments_and_tests() {
    let mut w = before();
    let contract = |w: &World, key: &str| objects::contract_revision(&w.state.objects[key]);
    let (ac, task) = (contract(&w, "ac:ios"), contract(&w, "task:t1"));
    let revision = w.state.objects["ac:ios"].current().revision;
    let boundaries = w.state.criterion_boundaries.clone();
    internal(&mut w, "migrate_body");
    let ios = data(&w, "ac:ios");
    assert_eq!(ios["body"], GWT);
    assert!(ios.get("description").is_none());
    assert_eq!(w.state.objects["ac:ios"].current().revision, revision + 1);
    assert_eq!(contract(&w, "ac:ios"), ac);
    assert_eq!(data(&w, "task:t1")["body"], "Keep the paid access");
    assert!(data(&w, "task:t1").get("contract").is_none());
    assert_eq!(contract(&w, "task:t1"), task);
    assert_eq!(
        w.state.task_process["task:t1"].contract,
        "Keep the paid access"
    );
    for ac in ["ac:ios", "ac:key"] {
        assert_eq!(
            objects::assessment(&w.state, ac).unwrap()["state"],
            "ready",
            "{ac}"
        );
    }
    assert_eq!(w.state.criterion_boundaries, boundaries);
    let rules = &objects::schema(&w.state).types["ac"].required_fields;
    assert!(rules.contains_key("body") && !rules.contains_key("description"));
    // The event runs once.
    let once = serde_json::to_value(&w.state).unwrap();
    internal(&mut w, "migrate_body");
    assert_eq!(serde_json::to_value(&w.state).unwrap(), once);
}

// body-text-ac2
#[test]
fn after_the_migration_a_write_names_its_text_body_and_changes_no_other_object() {
    let mut w = before();
    internal(&mut w, "migrate_body");
    let counts = |w: &World| -> Vec<(String, usize)> {
        let objects = w.state.objects.iter();
        objects
            .map(|(k, o)| (k.clone(), o.revisions.len()))
            .collect()
    };
    let old = counts(&w);
    let mut cap = data(&w, "capability:c");
    cap["body"] = json!("Paid access stays after a restart");
    save(&mut w, "capability:c", "domain:d", cap).unwrap();
    let changed: Vec<_> = counts(&w)
        .into_iter()
        .filter(|entry| !old.contains(entry))
        .map(|(k, _)| k)
        .collect();
    assert_eq!(changed, ["capability:c"]);
    assert_eq!(
        data(&w, "capability:c")["body"],
        "Paid access stays after a restart"
    );
    let mut ac = data(&w, "ac:key");
    ac["description"] = json!(GWT);
    let refused = save(&mut w, "ac:key", "requirement:r", ac).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "use body instead of description; read joppa_help"
    );
    let mut task = data(&w, "task:t1");
    task["contract"] = json!("Keep the access");
    let refused = save(&mut w, "task:t1", "ac:ios", task).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "use body instead of contract; read joppa_help"
    );
    let mut task = data(&w, "task:t1");
    task["body"] = json!("Keep the paid access on iOS");
    save(&mut w, "task:t1", "ac:ios", task).unwrap();
    assert_eq!(
        w.state.task_process["task:t1"].contract,
        "Keep the paid access on iOS"
    );
    let s = objects::schema(&w.state);
    let mut types = serde_json::to_value(&s.types).unwrap();
    types["ac"]["required_fields"] = json!({"contract":{"contains":["Given:"],"error":"Given"}});
    let refused = w
        .apply(
            "owner",
            json!({"op":"set_tree_schema","revision":s.revision,"types":types,
            "reason":"Require a contract"}),
        )
        .unwrap_err();
    assert_eq!(
        refused.to_string(),
        "ac: use body instead of contract; read joppa_help"
    );
}
