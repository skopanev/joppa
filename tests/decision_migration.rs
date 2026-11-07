//! migrate_decision_required removes data.decision_required once. Its revisions keep the
//! contract revision, and a live write refuses the field.
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
    ] {
        internal(&mut w, op);
    }
    assert!(!w.state.decision_required_migrated);
    w
}

// readiness-per-criterion-ac4
#[test]
fn the_migration_removes_the_flag_once_and_keeps_contracts() {
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
    }
    let records = serde_json::to_value(&w.state.requirements).unwrap();
    internal(&mut w, "migrate_decision_required");
    for (i, key) in keys.iter().enumerate() {
        let current = w.state.objects[*key].current();
        assert!(current.data.get(FIELD).is_none(), "{key}");
        assert_eq!(current.revision, revisions[i] + 1);
        assert_eq!(current.reason, joppa::decision_migration::REASON);
        assert_eq!(contract(&w, key), pins[i], "{key}");
    }
    // The stored criterion records stay: the trigger protect_state_revision guards them.
    assert_eq!(
        serde_json::to_value(&w.state.requirements).unwrap(),
        records
    );
    // The event runs once.
    let once = serde_json::to_value(&w.state).unwrap();
    internal(&mut w, "migrate_decision_required");
    assert_eq!(serde_json::to_value(&w.state).unwrap(), once);
}

// readiness-per-criterion-ac4
#[test]
fn a_live_write_and_a_type_rule_refuse_the_flag() {
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
}

// readiness-per-criterion-ac4
#[test]
fn a_save_after_the_migration_keeps_the_stored_owner() {
    let mut w = before();
    let save = |w: &World, data: serde_json::Value| {
        let revision = w.state.objects["ac:ios"].current().revision;
        json!({"op":"save_object","kind":"ac","id":"ios","revision":revision,
            "parent":"requirement:r","data":data,"reason":"Name the decision"})
    };
    // Before the migration, a decision of the requirement owner stores that owner.
    let mut data = w.state.objects["ac:ios"].current().data.clone();
    (data["owner"], data[FIELD]) = (json!("owner"), json!(true));
    w.ok("owner", save(&w, data));
    let stored = |w: &World| {
        let r = w.state.requirements["r"].revisions.last().unwrap();
        r.acs.iter().find(|a| a.id == "ios").unwrap().owner.clone()
    };
    assert_eq!(stored(&w).as_deref(), Some("owner"));
    internal(&mut w, "migrate_decision_required");
    // A new data value of the criterion keeps the stored owner.
    w.ok(
        "owner",
        json!({"op":"save_object","kind":"consumer","id":"buyer","revision":0,
        "parent":null,"data":{"title":"Buyer","owner":"owner"},"reason":"Add a consumer"}),
    );
    let mut data = w.state.objects["ac:ios"].current().data.clone();
    data["consumer"] = json!("consumer:buyer");
    w.ok("owner", save(&w, data));
    assert_eq!(stored(&w).as_deref(), Some("owner"));
}
