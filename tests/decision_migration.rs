//! requirement:readiness-per-criterion: an open question on a node is its readiness mark.
//! migrate_decision_required removes data.decision_required once. Its revisions keep the
//! contract revision and the marks, and a live write refuses the field.
mod common;
use common::*;
use joppa::decision_migration::{FIELD, REFUSAL};
use joppa::objects;
use serde_json::json;

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

/// A workspace before the event: migrate_documents gave each criterion the flag.
fn before() -> World {
    let mut w = World::new();
    w.ready();
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
    ] {
        internal(&mut w, op);
    }
    assert!(!w.state.decision_required_migrated);
    w
}

// readiness-per-criterion-ac4
#[test]
fn the_migration_removes_the_flag_once_and_keeps_contracts_and_marks() {
    let mut w = before();
    let keys = ["ac:ios", "ac:key"];
    let contract = |w: &World, key: &str| objects::contract_revision(&w.state.objects[key]);
    let pins: Vec<_> = keys.iter().map(|k| contract(&w, k)).collect();
    let revisions: Vec<_> = keys
        .iter()
        .map(|k| w.state.objects[*k].current().revision)
        .collect();
    for key in keys {
        assert!(
            w.state.objects[key].current().data.get(FIELD).is_some(),
            "{key}"
        );
        assert_eq!(
            objects::assessment(&w.state, key).unwrap()["state"],
            "ready"
        );
    }
    internal(&mut w, "migrate_decision_required");
    for (i, key) in keys.iter().enumerate() {
        let current = w.state.objects[*key].current();
        assert!(current.data.get(FIELD).is_none(), "{key}");
        assert_eq!(current.revision, revisions[i] + 1);
        assert_eq!(current.reason, joppa::decision_migration::REASON);
        assert_eq!(contract(&w, key), pins[i], "{key}");
        assert_eq!(
            objects::assessment(&w.state, key).unwrap()["state"],
            "ready"
        );
    }
    let r = w.state.requirements["r"].revisions.last().unwrap();
    assert!(
        r.acs.iter().all(|ac| ac.decision_required.is_none()),
        "{:?}",
        r.acs
    );
    // The event runs once.
    let once = serde_json::to_value(&w.state).unwrap();
    internal(&mut w, "migrate_decision_required");
    assert_eq!(serde_json::to_value(&w.state).unwrap(), once);
}

// readiness-per-criterion-ac4
#[test]
fn a_live_write_refuses_the_flag_and_an_open_question_is_a_waiting_mark() {
    let mut w = before();
    internal(&mut w, "migrate_decision_required");
    let mut ac = w.state.objects["ac:key"].current().data.clone();
    let revision = w.state.objects["ac:key"].current().revision;
    ac[FIELD] = json!(true);
    let save = json!({"op":"save_object","kind":"ac","id":"key","revision":revision,
        "parent":"requirement:r","data":ac,"reason":"Ask the owner"});
    assert_eq!(w.apply("owner", save).unwrap_err().to_string(), REFUSAL);
    let schema = objects::schema(&w.state);
    let mut rule = serde_json::to_value(&schema.types["ac"]).unwrap();
    rule["required_fields"][FIELD] = json!("a criterion needs a decision flag");
    let put = json!({"op":"set_tree_schema","revision":schema.revision,
        "put_types":{"ac":rule},"reason":"Require the flag"});
    assert_eq!(w.apply("owner", put).unwrap_err().to_string(), REFUSAL);
    let question = "OPEN QUESTION: Which limit applies?";
    w.ok(
        "agent",
        json!({"op":"assess","target":"ac:key","ready":false,"reason":question}),
    );
    let mark = objects::assessment(&w.state, "ac:key").unwrap();
    assert_eq!(
        (&mark["state"], &mark["reason"]),
        (&json!("waiting"), &json!(question))
    );
}
