mod common;
use common::*;
use joppa::{
    commands::{self, Command},
    model::Workspace,
    objects::{self, TreeType},
    tasks,
};
use serde_json::{Value, json};

fn save(w: &mut World, key: &str, parent: Option<&str>, data: Value) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok("agent",json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Set the document contract"}));
}
fn document(title: &str) -> Value {
    json!({"title":title,"owner":"owner"})
}
fn contract(title: &str) -> Value {
    json!({"title":title,"owner":"owner","contract":"Implement the specified document behavior","purpose":"implementation","projects":[]})
}
fn configured() -> World {
    let mut w = World {
        state: Workspace::default(),
        position: 0,
    };
    w.ok(
        "owner",
        json!({"op":"workspace","title":"Generic specifications"}),
    );
    w.ok("owner", json!({"op":"member","id":"agent","role":"agent"}));
    w.ok("owner", json!({"op":"member","id":"other","role":"agent"}));
    let mut types: std::collections::BTreeMap<String, TreeType> = std::collections::BTreeMap::new();
    for (kind, role, root, parents) in [
        ("spec", "document", true, vec!["spec"]),
        ("task", "task", false, vec!["*"]),
        ("work", "task", true, vec!["*"]),
    ] {
        types.insert(
            kind.into(),
            TreeType {
                index: None,
                references: Default::default(),
                parent_decision: false,
                inbox: None,
                launch_blocker: None,
                color: None,
                title_size: None,
                title: kind.into(),
                role: role.into(),
                root,
                parents: parents.into_iter().map(String::from).collect(),
                parent_gate: None,
            },
        );
    }
    w.ok("agent",json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Use specifications with any depth"}));
    save(&mut w, "spec:root", None, document("Root specification"));
    save(
        &mut w,
        "spec:child",
        Some("spec:root"),
        document("Nested specification"),
    );
    tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    w
}
fn action(w: &mut World, actor: &str, key: &str, op: &str, extra: Value) {
    let mut cmd = json!({"op":op,"id":key,"revision":w.state.objects[key].current().revision});
    for (key, value) in extra.as_object().unwrap() {
        cmd[key] = value.clone();
    }
    w.ok(actor, cmd);
}

// [task-serves-one-ac-ac1]
#[test]
fn task_uses_its_schema_parent_and_own_revision_without_req_or_ac() {
    let mut w = configured();
    save(
        &mut w,
        "task:one",
        Some("spec:child"),
        contract("Implement the child"),
    );
    save(
        &mut w,
        "work:two",
        Some("task:one"),
        contract("Implement a nested task"),
    );
    assert!(objects::within(&w.state, "work:two", "spec:root"));
    assert!(
        tasks::claimable(
            &w.state,
            "agent",
            "work:two",
            &w.state.task_process["work:two"]
        )
        .is_ok()
    );
    action(&mut w, "agent", "task:one", "claim_task", json!({}));
    w.no(
        "other",
        json!({"op":"claim_task","id":"task:one","revision":1}),
    );
    assert!(
        tasks::claimable(
            &w.state,
            "agent",
            "work:two",
            &w.state.task_process["work:two"]
        )
        .is_ok()
    );
    action(&mut w, "agent", "work:two", "claim_task", json!({}));
    action(
        &mut w,
        "agent",
        "work:two",
        "complete_task",
        json!({"evidence":"The child change is complete"}),
    );
    action(
        &mut w,
        "agent",
        "task:one",
        "complete_task",
        json!({"evidence":"The specified behavior works"}),
    );
    assert_eq!(objects::process(&w.state, "task:one")["status"], "done");
    assert_ne!(objects::signal(&w.state, "task:one")["tone"], "green");
    assert!(w.state.requirements.is_empty());
    assert_eq!(w.state.objects["task:one"].revisions.len(), 3);
    objects::validate_tree(&w.state).unwrap();
}

// [restructure-keeps-evidence-ac3]
#[test]
fn task_parent_change_preserves_id_and_requires_owner_review() {
    let mut w = configured();
    save(
        &mut w,
        "task:one",
        Some("spec:child"),
        contract("Implement the child"),
    );
    let before = w.state.objects["task:one"].revisions[0].clone();
    w.no("agent",json!({"op":"save_object","kind":"task","id":"one","revision":1,"parent":"spec:root","data":contract("Implement the child"),"reason":"Move the Task"}));
    w.ok("owner",json!({"op":"save_object","kind":"task","id":"one","revision":1,"parent":"spec:root","data":contract("Implement the child"),"reason":"Move the Task"}));
    assert!(
        tasks::claimable(
            &w.state,
            "agent",
            "task:one",
            &w.state.task_process["task:one"]
        )
        .is_err()
    );
    action(
        &mut w,
        "owner",
        "task:one",
        "review_task",
        json!({"reason":"The contract applies to its new parent"}),
    );
    action(&mut w, "agent", "task:one", "claim_task", json!({}));
    action(
        &mut w,
        "owner",
        "task:one",
        "release_task",
        json!({"reason":"Return the Task"}),
    );
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(&w.state.objects["task:one"].revisions[0]).unwrap()
    );
    save(
        &mut w,
        "spec:root",
        None,
        document("Updated root specification"),
    );
    assert!(tasks::review_required(
        &w.state,
        "task:one",
        &w.state.task_process["task:one"]
    ));
}

#[test]
fn dependencies_pin_generic_task_completion_and_reject_parent_cycles() {
    let mut w = configured();
    save(&mut w, "work:source", None, contract("Source task"));
    action(&mut w, "agent", "work:source", "claim_task", json!({}));
    action(
        &mut w,
        "agent",
        "work:source",
        "complete_task",
        json!({"evidence":"The source is complete"}),
    );
    let pin = w.state.objects["work:source"].current().revision;
    w.ok("agent",json!({"op":"save_object","kind":"task","id":"dependent","revision":0,"parent":"spec:root","data":contract("Dependent task"),"links":[{"target":"work:source","relation":"depends_on","revision":pin,"blocking":true}],"reason":"Depend on the source"}));
    assert!(
        tasks::claimable(
            &w.state,
            "agent",
            "task:dependent",
            &w.state.task_process["task:dependent"]
        )
        .is_ok()
    );
    action(
        &mut w,
        "owner",
        "work:source",
        "retire_task",
        json!({"reason":"The result no longer applies"}),
    );
    assert!(!objects::work_ready(&w.state, "task:dependent"));
    save(
        &mut w,
        "task:parent",
        Some("spec:root"),
        contract("Parent task"),
    );
    save(
        &mut w,
        "task:child",
        Some("task:parent"),
        contract("Child task"),
    );
    w.no("owner",json!({"op":"save_object","kind":"task","id":"parent","revision":1,"parent":"task:child","data":contract("Parent task"),"reason":"Create a parent cycle"}));
    w.no("agent",json!({"op":"save_object","kind":"task","id":"parent","revision":1,"parent":"spec:root","data":contract("Parent task"),"links":[{"target":"task:child","relation":"depends_on","revision":1,"blocking":true}],"reason":"Depend on the child"}));
}

#[test]
fn migration_preserves_old_task_history_and_uses_one_process() {
    let mut w = World::new();
    w.ready();
    w.ok("agent",json!({"op":"task","req":"r","revision":1,"id":"old","title":"Existing task","contract":"Implement the iOS flow","acs":["ios"],"projects":["app"],"purpose":"implementation"}));
    let old = serde_json::to_value(&w.state.objects["task:r/old"].revisions).unwrap();
    tasks::migrate(&mut w.state, "owner", w.position + 1, "later").unwrap();
    let migrated = serde_json::to_value(&w.state).unwrap();
    tasks::migrate(&mut w.state, "owner", w.position + 2, "later").unwrap();
    assert_eq!(migrated, serde_json::to_value(&w.state).unwrap());
    assert_eq!(
        old,
        serde_json::to_value(&w.state.objects["task:r/old"].revisions[..1]).unwrap()
    );
    action(&mut w, "agent", "task:r/old", "claim_task", json!({}));
    w.no(
        "other",
        json!({"op":"claim_task","req":"r","revision":1,"id":"old"}),
    );
    assert_eq!(
        tasks::task(&w.state, "task:r/old")
            .unwrap()
            .assignee
            .as_deref(),
        Some("agent")
    );
    let cmd: Command =
        serde_json::from_value(json!({"op":"confirm","req":"r","revision":1})).unwrap();
    commands::apply(&mut w.state, "owner", &cmd, w.position + 1, "later").unwrap();
    assert!(w.state.objects["task:r/old"].current().present);
}

#[test]
fn old_save_object_task_events_keep_their_original_replay_adapter() {
    let mut w = World::new();
    w.ready();
    let save = json!({"op":"save_object","kind":"task","id":"r/old-save","revision":0,"parent":"ac:ios","data":{"title":"Historical Task","contract":"Implement the iOS contract","projects":["app"],"purpose":"implementation"},"reason":"Create work"});
    w.ok("agent", save);
    assert!(
        w.state.requirements["r"]
            .revisions
            .last()
            .unwrap()
            .tasks
            .contains_key("old-save")
    );
    w.ok(
        "agent",
        json!({"op":"claim_task","req":"r","revision":1,"id":"old-save"}),
    );
    let original = serde_json::to_value(&w.state.objects["task:r/old-save"].revisions).unwrap();
    tasks::migrate(&mut w.state, "owner", w.position + 1, "later").unwrap();
    assert_eq!(
        original,
        serde_json::to_value(&w.state.objects["task:r/old-save"].revisions[..1]).unwrap()
    );
    assert_eq!(
        w.state.task_process["task:r/old-save"].assignee.as_deref(),
        Some("agent")
    );
}

#[test]
fn decision_task_owner_can_collect_the_configured_parent_decision() {
    let mut w = World::new();
    w.ready();
    tasks::migrate(&mut w.state, "owner", w.position + 1, "later").unwrap();
    save(
        &mut w,
        "task:decision",
        Some("ac:ios"),
        json!({"title":"Collect a decision","owner":"legal","contract":"Collect the parent owner's decision","purpose":"decision","projects":[]}),
    );
    action(&mut w, "legal", "task:decision", "claim_task", json!({}));
    let revision = w.state.objects["task:decision"].current().revision;
    w.no("legal",json!({"op":"complete_task","id":"task:decision","revision":revision,"evidence":"A free text decision"}));
    w.ok("owner",json!({"op":"comment","req":"r","revision":1,"id":"decision","text":"The iOS contract is approved","kind":"decision","ac":"ios","reply_to":null}));
    action(
        &mut w,
        "legal",
        "task:decision",
        "complete_task",
        json!({"evidence":"decision"}),
    );
    assert_eq!(
        objects::process(&w.state, "task:decision")["status"],
        "done"
    );
    assert_ne!(objects::signal(&w.state, "ac:ios")["tone"], "green");
}

#[test]
fn configured_verification_scope_applies_to_independent_task_projects() {
    let mut w = World::new();
    w.ready();
    tasks::migrate(&mut w.state, "owner", w.position + 1, "later").unwrap();
    w.ok("owner",json!({"op":"define_verification_scope","req":"r","revision":1,"subject_keys":["app"],"scope":"Verify the iOS application","reason":"Agree on the component boundary"}));
    let make = |projects: Value| json!({"op":"save_object","kind":"task","id":"scope","revision":0,"parent":"ac:ios","data":{"title":"Scoped work","owner":"owner","contract":"Implement the scoped behavior","purpose":"implementation","projects":projects},"reason":"Create scoped work"});
    w.no("agent", make(json!(["backend"])));
    w.ok("agent", make(json!(["app"])));
    w.no("agent",json!({"op":"amend_task","id":"task:scope","revision":1,"title":"Scoped work","contract":"Implement the scoped behavior","projects":["backend"],"reason":"Change the component"}));
    assert_eq!(w.state.task_process["task:scope"].projects, vec!["app"]);
}

// [one-taker-ac2]
#[test]
fn schema_selects_completion_or_test_evidence_as_the_parent_gate() {
    let mut w = configured();
    let mut schema = objects::schema(&w.state);
    schema.types.get_mut("task").unwrap().parent_gate = Some("completed".into());
    w.ok("agent",json!({"op":"set_tree_schema","revision":schema.revision,"types":schema.types,"reason":"Require completed parent work"}));
    save(
        &mut w,
        "work:parent",
        None,
        contract("Complete the parent work"),
    );
    save(
        &mut w,
        "task:child",
        Some("work:parent"),
        contract("Start after the parent work"),
    );
    assert!(
        tasks::claimable(
            &w.state,
            "agent",
            "task:child",
            &w.state.task_process["task:child"]
        )
        .is_err()
    );
    action(&mut w, "agent", "work:parent", "claim_task", json!({}));
    action(
        &mut w,
        "agent",
        "work:parent",
        "complete_task",
        json!({"evidence":"The parent is complete"}),
    );
    action(&mut w, "agent", "task:child", "claim_task", json!({}));

    let mut w = World::new();
    w.ready();
    tasks::migrate(&mut w.state, "owner", w.position + 1, "later").unwrap();
    let mut types = objects::defaults();
    types.get_mut("task").unwrap().parent_gate = Some("verified".into());
    w.ok("agent",json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Require verified parent evidence"}));
    save(
        &mut w,
        "task:verified",
        Some("ac:ios"),
        contract("Work after the tests pass"),
    );
    assert!(
        tasks::claimable(
            &w.state,
            "agent",
            "task:verified",
            &w.state.task_process["task:verified"]
        )
        .is_err()
    );
    w.run("verified-parent", "pass");
    action(&mut w, "agent", "task:verified", "claim_task", json!({}));
}

#[test]
fn custom_purpose_can_be_saved_and_claimed_without_changing_completion_rules() {
    let mut w = configured();
    let mut data = contract("Document the release");
    data["purpose"] = json!("documentation");
    save(&mut w, "work:custom", Some("spec:root"), data);
    action(&mut w, "agent", "work:custom", "claim_task", json!({}));
    assert_eq!(
        objects::process(&w.state, "work:custom")["status"],
        "in_progress"
    );
    assert_eq!(w.state.task_process["work:custom"].purpose, "documentation");
}
