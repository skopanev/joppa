//! The removed artificial rules: a dependency names a node and needs no revision, a Task
//! purpose is an optional label, the holder may move or hand over a Task, a criterion may have
//! its own owner without a decision, and a new required field binds only later saves.
mod common;
use common::*;
use joppa::{objects, tasks};
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

fn task(title: &str, owner: &str, purpose: Option<&str>) -> Value {
    let mut data = json!({"title":title,"owner":owner,"body":"Implement the agreed part"});
    if let Some(purpose) = purpose {
        data["purpose"] = json!(purpose);
    }
    data
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
        task("A", "owner", Some("implementation")),
        json!([]),
    )
    .unwrap();
    let link = json!([{"target":"task:a","relation":"depends_on","blocking":true}]);
    save(
        &mut w,
        "owner",
        "task:b",
        "ac:ios",
        task("B", "owner", Some("implementation")),
        link,
    )
    .unwrap();
    assert_eq!(w.state.objects["task:b"].current().links[0].revision, None);
    // A new text of A changes nothing for B: B waits until A is done.
    save(
        &mut w,
        "owner",
        "task:a",
        "ac:ios",
        task("A2", "owner", Some("implementation")),
        json!([]),
    )
    .unwrap();
    assert_eq!(
        objects::process(&w.state, "task:b")["review_required"],
        false
    );
    let claimable = |w: &World, key: &str| {
        tasks::claimable(&w.state, "agent", key, &w.state.task_process[key]).is_ok()
    };
    assert!(!claimable(&w, "task:b"));
    let revision = w.state.objects["task:a"].current().revision;
    w.ok(
        "agent",
        json!({"op":"claim_task","id":"task:a","revision":revision}),
    );
    let revision = w.state.objects["task:a"].current().revision;
    w.ok(
        "agent",
        json!({"op":"complete_task","id":"task:a","revision":revision,
        "evidence":"A is done","commit":"a".repeat(40)}),
    );
    assert!(objects::work_ready(&w.state, "task:b"));
}

// loose-rules-ac2
#[test]
fn a_task_purpose_is_optional_and_the_holder_moves_or_hands_over_a_task() {
    let mut w = current();
    save(
        &mut w,
        "owner",
        "task:p",
        "ac:ios",
        task("P", "owner", None),
        json!([]),
    )
    .unwrap();
    assert_eq!(w.state.task_process["task:p"].purpose, "");
    save(
        &mut w,
        "owner",
        "task:p",
        "ac:ios",
        task("P", "owner", Some("documentation")),
        json!([]),
    )
    .unwrap();
    assert_eq!(w.state.task_process["task:p"].purpose, "documentation");
    // The holder, who is not the owner, moves the Task and hands it over.
    let revision = w.state.objects["task:p"].current().revision;
    w.ok(
        "agent",
        json!({"op":"claim_task","id":"task:p","revision":revision}),
    );
    save(
        &mut w,
        "agent",
        "task:p",
        "ac:key",
        task("P", "legal", Some("documentation")),
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
    ac["decision_required"] = json!(false);
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
        (&data["owner"], &data["decision_required"]),
        (&json!("legal"), &json!(false))
    );
    // No decision: an agent assesses the criterion as ready without a linked decision.
    let number = w.state.requirements["r"].revisions.last().unwrap().number;
    w.ok(
        "agent",
        json!({"op":"assess","req":"r","revision":number,"ac":"key",
        "assessment":{"ready":true,"reason":"Known scope"}}),
    );
    assert_eq!(
        objects::assessment(&w.state, "ac:key").unwrap()["state"],
        "ready"
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
