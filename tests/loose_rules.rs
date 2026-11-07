//! The loose rules: a dependency names a node and needs no revision, a Task has no purpose, a
//! member may move or hand over a Task, a criterion may have its own owner without a decision,
//! and a new required field binds only later saves.
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

/// A current workspace: every migration ran, and both criteria are ready.
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

fn save(
    w: &mut World,
    actor: &str,
    key: &str,
    parent: &str,
    data: Value,
    links: Value,
) -> anyhow::Result<()> {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.apply(
        actor,
        json!({"op":"save_object","kind":kind,"id":id,"revision":revision,
        "parent":parent,"data":data,"links":links,"reason":"Change the work"}),
    )
}

fn task(title: &str, owner: &str) -> Value {
    json!({"title":title,"owner":owner,"body":"Implement the agreed part"})
}

// loose-rules-ac1
#[test]
fn a_dependency_names_a_node_and_a_new_target_revision_asks_for_no_review() {
    let mut w = current();
    save(
        &mut w,
        "owner",
        "task:a",
        "ac:ios",
        task("A", "owner"),
        json!([]),
    )
    .unwrap();
    let link = json!([{"target":"task:a","relation":"depends_on","blocking":true}]);
    save(
        &mut w,
        "owner",
        "task:b",
        "ac:ios",
        task("B", "owner"),
        link,
    )
    .unwrap();
    assert_eq!(w.state.objects["task:b"].current().links[0].revision, None);
    // A new text of A changes nothing for B: its link and its process stay the same.
    let before = objects::process(&w.state, "task:b");
    save(
        &mut w,
        "owner",
        "task:a",
        "ac:ios",
        task("A2", "owner"),
        json!([]),
    )
    .unwrap();
    assert_eq!(objects::process(&w.state, "task:b"), before);
    assert_eq!(w.state.objects["task:b"].current().links[0].revision, None);
    assert_eq!(w.state.objects["task:b"].revisions.len(), 1);
}

// loose-rules-ac2
#[test]
fn a_task_has_no_purpose_and_a_member_moves_or_hands_over_a_task() {
    let mut w = current();
    save(
        &mut w,
        "owner",
        "task:p",
        "ac:ios",
        task("P", "owner"),
        json!([]),
    )
    .unwrap();
    assert!(
        w.state.objects["task:p"]
            .current()
            .data
            .get("purpose")
            .is_none()
    );
    let mut labelled = task("P", "owner");
    labelled["purpose"] = json!("documentation");
    let error = save(&mut w, "owner", "task:p", "ac:ios", labelled, json!([])).unwrap_err();
    assert_eq!(error.to_string(), joppa::purpose_migration::REFUSAL);
    // A member who is not the owner moves the Task and hands it over.
    save(
        &mut w,
        "agent",
        "task:p",
        "ac:key",
        task("P", "legal"),
        json!([]),
    )
    .unwrap();
    let p = w.state.objects["task:p"].current();
    assert_eq!(p.parent.as_deref(), Some("ac:key"));
    assert_eq!(p.data["owner"], "legal");
}

// loose-rules-ac3
#[test]
fn a_criterion_has_its_own_owner_without_a_decision_and_a_new_rule_binds_later_saves() {
    let mut w = current();
    let mut ac = w.state.objects["ac:key"].current().data.clone();
    ac["owner"] = json!("legal");
    save(
        &mut w,
        "owner",
        "ac:key",
        "requirement:r",
        ac.clone(),
        json!([]),
    )
    .unwrap();
    let data = &w.state.objects["ac:key"].current().data;
    assert_eq!(
        (&data["owner"], data.get("decision_required")),
        (&json!("legal"), None)
    );
    // A new required field does not refuse the stored objects; the next save needs it.
    let s = objects::schema(&w.state);
    let mut types = serde_json::to_value(&s.types).unwrap();
    types["ac"]["required_fields"] = json!({"body":{"contains":["Given:"],"error":"Given"}});
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":s.revision,"types":types,
        "reason":"Require Given"}),
    );
    ac["body"] = json!("No marker");
    let refused = save(&mut w, "owner", "ac:key", "requirement:r", ac, json!([])).unwrap_err();
    assert_eq!(refused.to_string(), "Given");
}
