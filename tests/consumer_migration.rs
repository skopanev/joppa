//! One consumer per criterion: migrate_consumers moves the consumers of a workspace from
//! its requirements to a single required reference of each criterion.
mod common;
use common::*;
use serde_json::json;

/// Apply an internal migration event, as the journal replay does.
fn internal(w: &mut World, actor: &str, op: &str) -> anyhow::Result<()> {
    let command: joppa::commands::Command = serde_json::from_value(json!({ "op": op }))?;
    let mut copy = w.state.clone();
    joppa::commands::apply_replay(
        &mut copy,
        actor,
        &command,
        w.position + 1,
        "2026-01-01T00:00:00Z",
    )?;
    w.state = copy;
    w.position += 1;
    Ok(())
}

/// A workspace before the migration: its criteria list consumers, its requirement too.
fn before() -> World {
    let mut w = World::new();
    // The live order of the earlier migrations.
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
    ] {
        internal(&mut w, "owner", op).unwrap();
    }
    w.ok(
        "owner",
        json!({"op":"consumer","id":"agentc","title":"Agent of a user","owner":"owner"}),
    );
    let mut schema = joppa::objects::schema(&w.state);
    schema
        .types
        .retain(|kind, _| !["intake", "point"].contains(&kind.as_str()));
    schema.types.get_mut("ac").unwrap().references.insert(
        "consumers".into(),
        joppa::objects::ReferenceRule {
            target: "consumer".into(),
            title: "For".into(),
            required: false,
            multiple: true,
            verification_subject: false,
        },
    );
    w.ok("owner", json!({"op":"set_tree_schema","revision":schema.revision,"types":schema.types,"reason":"Criteria list their consumers"}));
    let ios = w.state.objects["ac:ios"].current().clone();
    let mut data = ios.data.clone();
    data["consumers"] = json!(["agentc", "customer"]);
    w.ok("owner", json!({"op":"save_object","kind":"ac","id":"ios","revision":ios.revision,"parent":ios.parent,"data":data,"reason":"Name two consumers"}));
    w
}

// one-consumer-per-criterion-ac1
#[test]
fn each_criterion_takes_one_consumer_and_keeps_its_status() {
    let mut w = before();
    let (ios, key) = (
        w.state.objects["ac:ios"].current().revision,
        w.state.objects["ac:key"].current().revision,
    );
    let boundaries = w.state.criterion_boundaries.clone();
    let schema = joppa::objects::schema(&w.state).revision;
    internal(&mut w, "owner", "migrate_consumers").unwrap();
    let types = joppa::objects::schema(&w.state);
    assert_eq!(types.revision, schema + 1);
    let rule = &types.types["ac"].references["consumer"];
    assert!(rule.required && !rule.multiple && rule.target == "consumer");
    assert!(!types.types["ac"].references.contains_key("consumers"));
    assert!(
        types.types["requirement"]
            .references
            .values()
            .all(|r| r.target != "consumer")
    );
    // The first valid consumer of the criterion, else the first one of its requirement.
    let ios_now = w.state.objects["ac:ios"].current();
    assert_eq!(ios_now.data["consumer"], "agentc");
    assert!(ios_now.data.get("consumers").is_none());
    assert_eq!(ios_now.revision, ios + 1);
    let key_now = w.state.objects["ac:key"].current();
    assert_eq!(key_now.data["consumer"], "customer");
    assert_eq!(key_now.revision, key + 1);
    // The text, the tests and the readiness do not change, so no criterion resets.
    assert_eq!(w.state.criterion_boundaries, boundaries);
    // A second migration changes nothing.
    let state = serde_json::to_value(&w.state).unwrap();
    internal(&mut w, "owner", "migrate_consumers").unwrap();
    assert_eq!(serde_json::to_value(&w.state).unwrap(), state);
}

// one-consumer-per-criterion-ac2
#[test]
fn after_the_migration_a_criterion_save_needs_one_consumer() {
    let mut w = before();
    internal(&mut w, "owner", "migrate_consumers").unwrap();
    let ios = w.state.objects["ac:ios"].current().clone();
    let save = |data: serde_json::Value| json!({"op":"save_object","kind":"ac","id":"ios","revision":ios.revision,"parent":ios.parent,"data":data,"reason":"Change the consumer"});
    let mut data = ios.data.clone();
    data.as_object_mut().unwrap().remove("consumer");
    w.no("owner", save(data.clone()));
    data["consumer"] = json!(["agentc", "customer"]);
    w.no("owner", save(data.clone()));
    data["consumer"] = json!("customer");
    w.ok("owner", save(data));
    assert_eq!(
        w.state.objects["ac:ios"].current().data["consumer"],
        "customer"
    );
}

// one-consumer-per-criterion-ac1
#[test]
fn only_the_workspace_owner_runs_the_migration() {
    let mut w = before();
    assert!(internal(&mut w, "agent", "migrate_consumers").is_err());
    assert!(!w.state.consumers_migrated);
}

// one-consumer-per-criterion-ac3
#[test]
fn a_legacy_revise_gives_a_new_criterion_the_first_consumer_of_its_requirement() {
    let mut w = World::new();
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
        "migrate_consumers",
    ] {
        internal(&mut w, "owner", op).unwrap();
    }
    w.ok("owner", json!({"op":"revise","req":"n","expected_revision":0,"capability":"c","consumers":["customer"],"title":"New rule","body":"Scope","owner":"owner","reason":"Agreed","acs":[{"id":"fresh","text":"The fresh criterion holds"}]}));
    let fresh = w
        .state
        .objects
        .get("ac:fresh")
        .map(|o| o.current().data.clone());
    assert_eq!(
        fresh.as_ref().map(|d| d["consumer"].clone()),
        Some(json!("customer")),
        "{fresh:?}"
    );
}

// one-consumer-per-criterion-ac1
#[test]
fn a_later_requirement_save_keeps_the_consumer_of_each_criterion() {
    let mut w = before();
    internal(&mut w, "owner", "migrate_consumers").unwrap();
    let r = w.state.objects["requirement:r"].current().clone();
    let mut data = r.data.clone();
    data["body"] = json!("Only iOS and the web are in scope");
    w.ok("owner", json!({"op":"save_object","kind":"requirement","id":"r","revision":r.revision,"parent":r.parent,"data":data,"reason":"Widen the scope"}));
    assert_eq!(
        w.state.objects["ac:ios"].current().data["consumer"],
        "agentc"
    );
    assert_eq!(
        w.state.objects["ac:key"].current().data["consumer"],
        "customer"
    );
}
