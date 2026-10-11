mod common;
use common::World;
use joppa::{
    commands::{self, Command},
    model::Workspace,
    objects, tasks,
};
use serde_json::{Value, json};

fn configured() -> World {
    let mut w = World {
        state: Workspace::default(),
        position: 0,
    };
    w.ok(
        "owner",
        json!({"op":"workspace","title":"Planning contracts"}),
    );
    for actor in ["planner", "other", "worker"] {
        w.ok("owner", json!({"op":"member","id":actor,"role":"agent"}));
    }
    w.ok(
        "planner",
        json!({"op":"set_tree_schema","revision":0,"reason":"Configure roles","types":{
            "spec":{"title":"Specification","role":"requirement","root":true,"parents":[]},
            "criterion":{"title":"Criterion","role":"ac","parents":["spec"]},
            "work":{"title":"Work","role":"task","parents":["criterion"],"parent_gate":"ready"}
        }}),
    );
    objects::migrate_schema(&mut w.state, "owner", w.position + 1, "now").unwrap();
    tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    objects::migrate_documents(&mut w.state, "owner", w.position + 1, "now").unwrap();
    save(
        &mut w,
        "planner",
        "spec:one",
        None,
        json!({"title":"One rule","body":"Implement the agreed rule","owner":"owner"}),
    );
    for ac in ["criterion:a", "criterion:b"] {
        save(
            &mut w,
            "planner",
            ac,
            Some("spec:one"),
            json!({"title":ac,"owner":"owner"}),
        );
    }
    let revision = w.state.requirements["spec:one"]
        .revisions
        .last()
        .unwrap()
        .number;
    w.ok(
        "owner",
        json!({"op":"confirm","req":"spec:one","revision":revision}),
    );
    for ac in ["criterion:a", "criterion:b"] {
        w.ok("planner", json!({"op":"assess","req":"spec:one","revision":revision,"ac":ac,"assessment":{"ready":true,"reason":"Known scope"}}));
    }
    w
}
fn save(w: &mut World, actor: &str, key: &str, parent: Option<&str>, data: Value) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok(actor, json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Set the contract"}));
}
fn work(w: &mut World, actor: &str, id: &str) {
    save(
        w,
        actor,
        id,
        Some("criterion:a"),
        json!({"title":id,"owner":"owner","contract":"Implement this criterion","purpose":"implementation"}),
    );
}
fn claim(w: &mut World, actor: &str, ac: &str) {
    w.ok(
        actor,
        json!({"op":"claim_planning","ac":ac,"revision":w.state.objects[ac].current().revision}),
    );
}
fn refuse(w: &mut World, actor: &str, cmd: Value, expected: &str) {
    let before = serde_json::to_value(&w.state).unwrap();
    let position = w.position;
    assert_eq!(w.apply(actor, cmd).unwrap_err().to_string(), expected);
    assert_eq!(w.position, position);
    assert_eq!(serde_json::to_value(&w.state).unwrap(), before);
}

// planning-claim-ac1
#[test]
fn claim_guards_are_atomic_and_existing_work_does_not_block() {
    let mut w = configured();
    work(&mut w, "planner", "work:old");
    let revision = w.state.objects["criterion:a"].current().revision;
    refuse(
        &mut w,
        "planner",
        json!({"op":"claim_planning","ac":"criterion:a","revision":revision-1}),
        "stale object revision; read the object again",
    );
    refuse(
        &mut w,
        "planner",
        json!({"op":"claim_planning","ac":"criterion:missing","revision":1}),
        "unknown criterion",
    );
    refuse(
        &mut w,
        "planner",
        json!({"op":"claim_planning","ac":"work:old","revision":1}),
        "planning requires a criterion",
    );
    claim(&mut w, "planner", "criterion:a");
    assert_eq!(
        objects::process(&w.state, "criterion:a")["planning_claim"],
        json!({"actor":"planner","position":w.position})
    );
    refuse(
        &mut w,
        "other",
        json!({"op":"claim_planning","ac":"criterion:a","revision":revision}),
        "criterion is already claimed for planning",
    );
    let revision = w.state.objects["criterion:b"].current().revision;
    refuse(
        &mut w,
        "planner",
        json!({"op":"claim_planning","ac":"criterion:b","revision":revision}),
        "agent already holds a planning claim in this workspace",
    );
    assert!(objects::process(&w.state, "criterion:b")["planning_claim"].is_null());
    let mut other_workspace = configured();
    claim(&mut other_workspace, "planner", "criterion:b");
}

// planning-claim-ac2, planning-claim-ac3
#[test]
fn publication_gates_only_new_tasks_and_finish_preserves_existing_holder() {
    let mut w = configured();
    work(&mut w, "planner", "work:old");
    let revision = w.state.objects["work:old"].current().revision;
    w.ok(
        "worker",
        json!({"op":"claim_task","id":"work:old","revision":revision}),
    );
    claim(&mut w, "planner", "criterion:a");
    work(&mut w, "planner", "work:new");
    let t = &w.state.task_process["work:new"];
    assert_eq!(
        tasks::claimable(&w.state, "other", "work:new", t)
            .unwrap_err()
            .to_string(),
        "Task is awaiting planning publication"
    );
    refuse(
        &mut w,
        "other",
        json!({"op":"claim_task","id":"work:new","revision":1}),
        "Task is awaiting planning publication",
    );
    refuse(
        &mut w,
        "other",
        json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"}),
        "planning holder required",
    );
    let verification = objects::verification(&w.state, "criterion:a");
    w.ok(
        "planner",
        json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"}),
    );
    assert!(
        tasks::claimable(
            &w.state,
            "other",
            "work:new",
            &w.state.task_process["work:new"]
        )
        .is_ok()
    );
    assert_eq!(
        w.state.task_process["work:old"].assignee.as_deref(),
        Some("worker")
    );
    assert_eq!(objects::verification(&w.state, "criterion:a"), verification);
    assert!(objects::process(&w.state, "criterion:a")["planning_claim"].is_null());
}

// planning-claim-ac3
#[test]
fn finish_requires_changes_and_accepts_updates_without_new_tasks() {
    let mut w = configured();
    work(&mut w, "planner", "work:old");
    claim(&mut w, "planner", "criterion:a");
    refuse(
        &mut w,
        "planner",
        json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"}),
        "planning produced no Task changes",
    );
    work(&mut w, "planner", "work:old");
    w.ok(
        "planner",
        json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"}),
    );
}

// planning-claim-ac3
#[test]
fn finish_checks_missing_and_cyclic_dependencies_in_existing_reachable_work() {
    let mut w = configured();
    for id in ["work:old", "work:linked"] {
        work(&mut w, "planner", id);
    }
    claim(&mut w, "planner", "criterion:a");
    work(&mut w, "planner", "work:new");
    let link = |target: &str| objects::ObjectLink {
        target: target.into(),
        relation: "depends_on".into(),
        revision: Some(1),
        blocking: false,
    };
    // Simulate older invalid state. Current save_object already refuses these links.
    w.state
        .objects
        .get_mut("work:new")
        .unwrap()
        .revisions
        .last_mut()
        .unwrap()
        .links = vec![link("work:old")];
    w.state
        .objects
        .get_mut("work:old")
        .unwrap()
        .revisions
        .last_mut()
        .unwrap()
        .links = vec![link("work:missing")];
    refuse(
        &mut w,
        "planner",
        json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"}),
        "planning dependency target is missing",
    );
    w.state
        .objects
        .get_mut("work:old")
        .unwrap()
        .revisions
        .last_mut()
        .unwrap()
        .links = vec![link("work:linked")];
    w.state
        .objects
        .get_mut("work:linked")
        .unwrap()
        .revisions
        .last_mut()
        .unwrap()
        .links = vec![link("work:old")];
    refuse(
        &mut w,
        "planner",
        json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"}),
        "planning dependencies contain a cycle",
    );
}

// planning-claim-ac4, owner-releases-claim-ac2, returned-work-kept-ac1
#[test]
fn release_retires_new_work_and_preserves_existing_claim_and_document() {
    let mut w = configured();
    work(&mut w, "planner", "work:old");
    w.ok(
        "worker",
        json!({"op":"claim_task","id":"work:old","revision":1}),
    );
    claim(&mut w, "planner", "criterion:a");
    work(&mut w, "planner", "work:old");
    work(&mut w, "planner", "work:new");
    work(&mut w, "other", "work:other-new");
    let existing = serde_json::to_value(&w.state.objects["work:old"]).unwrap();
    refuse(
        &mut w,
        "other",
        json!({"op":"release_planning","ac":"criterion:a","reason":"Withdrawn"}),
        "workspace owner required",
    );
    refuse(
        &mut w,
        "planner",
        json!({"op":"release_planning","ac":"criterion:a","reason":" "}),
        "text must not be empty",
    );
    w.ok(
        "owner",
        json!({"op":"release_planning","ac":"criterion:a","reason":"Planner unavailable"}),
    );
    assert_eq!(
        w.state.task_process["work:new"].retired.as_deref(),
        Some("Planner unavailable")
    );
    assert!(w.state.objects["work:new"].current().present);
    assert!(w.state.task_process["work:other-new"].retired.is_none());
    assert_eq!(
        w.state.task_process["work:old"].assignee.as_deref(),
        Some("worker")
    );
    assert_eq!(
        serde_json::to_value(&w.state.objects["work:old"]).unwrap(),
        existing
    );
    refuse(
        &mut w,
        "owner",
        json!({"op":"release_planning","ac":"criterion:a","reason":"Withdrawn"}),
        "criterion has no planning claim",
    );
    claim(&mut w, "planner", "criterion:a");
    w.ok(
        "planner",
        json!({"op":"release_planning","ac":"criterion:a","reason":"No new work"}),
    );
}

// planning-claim-ac5
#[test]
fn journal_replay_and_serialization_keep_the_planning_claim_and_gate() {
    let mut w = configured();
    let before = w.state.clone();
    let position = w.position;
    let command: Command = serde_json::from_value(json!({"op":"claim_planning","ac":"criterion:a","revision":w.state.objects["criterion:a"].current().revision})).unwrap();
    claim(&mut w, "planner", "criterion:a");
    let mut replay = before;
    commands::apply_replay(
        &mut replay,
        "planner",
        &command,
        position + 1,
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    assert_eq!(replay.planning_claims, w.state.planning_claims);
    work(&mut w, "planner", "work:new");
    let restored: Workspace =
        serde_json::from_value(serde_json::to_value(&w.state).unwrap()).unwrap();
    assert_eq!(restored.planning_claims, w.state.planning_claims);
    assert!(joppa::planning::awaiting_publication(
        &restored,
        "work:new",
        &restored.task_process["work:new"]
    ));
}

// planning-claim-ac1
#[test]
fn readiness_guard_preserves_the_criterion_and_legacy_results_do_not_replace_junit() {
    let mut w = configured();
    let revision = w.state.requirements["spec:one"]
        .revisions
        .last()
        .unwrap()
        .number;
    let object_revision = w.state.objects["criterion:a"].current().revision;
    w.ok("planner", json!({"op":"assess","req":"spec:one","revision":revision,"ac":"criterion:a","assessment":{"ready":false,"reason":"Unknown input","recipient":"owner"}}));
    refuse(
        &mut w,
        "planner",
        json!({"op":"claim_planning","ac":"criterion:a","revision":object_revision}),
        "criterion is not ready for planning",
    );
    w.ok("planner", json!({"op":"assess","req":"spec:one","revision":revision,"ac":"criterion:a","assessment":{"ready":true,"reason":"Known input"}}));
    w.ok(
        "owner",
        json!({"op":"member","id":"runner","role":"runner"}),
    );
    w.ok("owner", json!({"op":"define_verification_scope","req":"spec:one","revision":revision,"subject_keys":["build"],"scope":"Criterion checks","reason":"Define the build"}));
    w.ok("runner", json!({"op":"plan","req":"spec:one","revision":revision,"run":"checks","subject":{"build":"commit-a"},"scope":"Criterion checks","discovery_complete":true,"checks":[
        {"id":"[criterion:a] exact result","ac":"criterion:a","method":"automated","expected":["expected result"]},
        {"id":"[criterion:b] exact result","ac":"criterion:b","method":"automated","expected":["expected result"]}
    ]}));
    for id in ["[criterion:a] exact result", "[criterion:b] exact result"] {
        w.ok("runner", json!({"op":"result","req":"spec:one","revision":revision,"run":"checks","evidence":{"check":id,"outcome":"pass","observed":["expected result"],"note":"Executed the check"}}));
    }
    w.ok(
        "runner",
        json!({"op":"finish","req":"spec:one","revision":revision,"run":"checks"}),
    );
    assert_eq!(objects::verification(&w.state, "criterion:a"), "pass");
    // JUnit status is checked by the transactional store, not by legacy run metadata.
    w.ok(
        "planner",
        json!({"op":"claim_planning","ac":"criterion:a","revision":object_revision}),
    );
    assert_eq!(w.state.planning_claims["criterion:a"].actor, "planner");
}

// planning-claim-ac3
#[test]
fn planning_holder_cannot_move_a_task_or_transfer_its_document_owner() {
    let mut w = configured();
    work(&mut w, "other", "work:existing");
    claim(&mut w, "planner", "criterion:a");
    let revision = w.state.objects["work:existing"].current().revision;
    let data = w.state.objects["work:existing"].current().data.clone();
    refuse(
        &mut w,
        "planner",
        json!({"op":"save_object","kind":"work","id":"existing","revision":revision,"parent":"criterion:b","data":data,"reason":"Move work"}),
        "Task owner required to change its parent",
    );
    let mut data = w.state.objects["work:existing"].current().data.clone();
    w.ok(
        "owner",
        json!({"op":"member","id":"new-owner","role":"human"}),
    );
    data["owner"] = json!("new-owner");
    refuse(
        &mut w,
        "planner",
        json!({"op":"save_object","kind":"work","id":"existing","revision":revision,"parent":"criterion:a","data":data,"reason":"Transfer work"}),
        "only current owner can transfer responsibility",
    );
    work(&mut w, "planner", "work:existing");
}

#[test]
fn an_active_planning_claim_prevents_archiving_the_criterion_or_its_ancestors() {
    let mut w = configured();
    claim(&mut w, "planner", "criterion:a");
    for target in ["criterion:a", "spec:one"] {
        let revision = w.state.objects[target].current().revision;
        refuse(
            &mut w,
            "owner",
            json!({"op":"archive_object","target":target,"revision":revision,"reason":"Archive the object"}),
            "cannot archive an object with an active planning claim",
        );
    }
    w.ok(
        "planner",
        json!({"op":"release_planning","ac":"criterion:a","reason":"Plan withdrawn"}),
    );
    let revision = w.state.objects["criterion:a"].current().revision;
    w.ok("owner", json!({"op":"archive_object","target":"criterion:a","revision":revision,"reason":"Archive after release"}));
}
