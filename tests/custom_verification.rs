mod common;
use common::*;
use joppa::{
    commands::{self, Command},
    model::Workspace,
    objects, tasks,
};
use serde_json::{Value, json};

fn save(w: &mut World, address: &str, parent: Option<&str>, data: Value) {
    let (kind, id) = address.split_once(':').unwrap();
    let revision = w
        .state
        .objects
        .get(address)
        .map_or(0, |o| o.current().revision);
    w.ok("agent",json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Set the specification contract"}));
}
fn setup_custom() -> World {
    let mut w = World {
        state: Workspace::default(),
        position: 0,
    };
    w.ok(
        "owner",
        json!({"op":"workspace","title":"Configured workflows"}),
    );
    w.ok("owner", json!({"op":"member","id":"agent","role":"agent"}));
    w.ok(
        "owner",
        json!({"op":"member","id":"runner","role":"runner"}),
    );
    let types = json!({
        "spec":{"title":"Spec","role":"requirement","root":true,"parents":["*"]},
        "epic":{"title":"Epic","role":"requirement","root":true,"parents":["*"]},
        "section":{"title":"Section","role":"group","root":true,"parents":["*"]},
        "measure":{"title":"Measure","role":"ac","root":false,"parents":["*"]},
        "rule":{"title":"Rule","role":"ac","root":false,"parents":["*"]},
        "work":{"title":"Work","role":"task","root":false,"parents":["*"],"parent_gate":"ready"},
        "source":{"title":"Input","role":"document","root":true,"parents":["*"],"inbox":true}
    });
    w.ok("agent",json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Configure all type names"}));
    objects::migrate_schema(&mut w.state, "owner", w.position + 1, "now").unwrap();
    tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    w
}
fn revision(w: &World, req: &str) -> u32 {
    w.state.requirements[req].revisions.last().unwrap().number
}
fn ready(w: &mut World, req: &str, criteria: &[&str]) {
    let revision = revision(w, req);
    w.ok(
        "owner",
        json!({"op":"confirm","req":req,"revision":revision}),
    );
    w.ok("owner",json!({"op":"define_verification_scope","req":req,"revision":revision,"subject_keys":["build"],"scope":"Configured specification tests","reason":"Define the build"}));
    for ac in criteria {
        w.ok("agent",json!({"op":"assess","req":req,"revision":revision,"ac":ac,"assessment":{"ready":true,"reason":"Contract is clear"}}));
    }
}
fn run(w: &mut World, req: &str, id: &str, criteria: &[(&str, &str)]) {
    let revision = revision(w, req);
    let checks:Vec<Value>=criteria.iter().map(|(ac,_)|json!({"id":format!("[{ac}] exact result"),"ac":ac,"method":"automated","expected":["expected result"]})).collect();
    w.ok("runner",json!({"op":"plan","req":req,"revision":revision,"run":id,"subject":{"build":"sha-one"},"scope":"Configured specification tests","discovery_complete":true,"checks":checks}));
    for (ac, outcome) in criteria {
        w.ok("runner",json!({"op":"result","req":req,"revision":revision,"run":id,"evidence":{"check":format!("[{ac}] exact result"),"outcome":outcome,"observed":["expected result"],"note":"Executed the exact test"}}));
    }
    w.ok(
        "runner",
        json!({"op":"finish","req":req,"revision":revision,"run":id}),
    );
}
#[test]
fn configured_types_verify_without_default_type_names_and_keep_ids_separate() {
    let mut w = setup_custom();
    for address in ["spec:same", "epic:same"] {
        save(
            &mut w,
            address,
            None,
            json!({"title":address,"body":"The common specification","owner":"owner","rationale":{"source":"Input"}}),
        );
    }
    save(
        &mut w,
        "section:between",
        Some("spec:same"),
        json!({"title":"Intermediate group","owner":"owner"}),
    );
    for address in ["measure:same", "rule:same"] {
        save(
            &mut w,
            address,
            Some("section:between"),
            json!({"title":address,"owner":"owner","description":"The expected observable result"}),
        );
    }
    assert!(!w.state.objects.contains_key("requirement:spec:same"));
    assert!(!w.state.objects.contains_key("ac:measure:same"));
    assert_eq!(
        objects::criterion_context(&w.state, "measure:same")
            .unwrap()
            .0,
        "spec:same"
    );
    assert_eq!(
        w.state.requirements["epic:same"]
            .revisions
            .last()
            .unwrap()
            .acs
            .len(),
        0
    );
    ready(&mut w, "spec:same", &["measure:same", "rule:same"]);
    save(
        &mut w,
        "work:implement",
        Some("measure:same"),
        json!({"title":"Implement criterion","owner":"owner","contract":"Implement the specified behavior","purpose":"implementation","details":{"file":"src/main.rs"}}),
    );
    let cmd = json!({"op":"claim_task","id":"work:implement","revision":1});
    w.ok("agent", cmd);
    w.ok("agent",json!({"op":"complete_task","id":"work:implement","revision":2,"evidence":"The change exists"}));
    assert_eq!(
        objects::verification(&w.state, "measure:same"),
        "unverified"
    );
    run(
        &mut w,
        "spec:same",
        "mixed",
        &[("measure:same", "pass"), ("rule:same", "fail")],
    );
    assert_eq!(objects::verification(&w.state, "measure:same"), "pass");
    assert_eq!(objects::verification(&w.state, "rule:same"), "fail");
    assert_eq!(objects::verification(&w.state, "spec:same"), "fail");
    assert_eq!(objects::verification(&w.state, "epic:same"), "unverified");
    assert_eq!(
        w.state.objects["spec:same"].current().data["rationale"]["source"],
        "Input"
    );
    assert_eq!(
        w.state.objects["work:implement"].current().data["details"]["file"],
        "src/main.rs"
    );
    run(
        &mut w,
        "spec:same",
        "green",
        &[("measure:same", "pass"), ("rule:same", "pass")],
    );
    let rev = revision(&w, "spec:same");
    w.ok(
        "owner",
        json!({"op":"accept","req":"spec:same","revision":rev,"run":"green"}),
    );
    assert_eq!(objects::verification(&w.state, "spec:same"), "pass");
    assert_eq!(
        objects::process(&w.state, "measure:same")["criterion"],
        "measure:same"
    );
    assert_eq!(
        objects::annotation_address(&w.state, "measure:same"),
        "measure:same"
    );
    w.ok(
        "agent",
        json!({"op":"note","target":"measure:same","id":"context","text":"Criterion context"}),
    );
    assert!(w.state.notes.contains_key("measure:same"));
    let old = w.state.objects["spec:same"].revisions.clone();
    let mut data = w.state.objects["spec:same"].current().data.clone();
    data["body"] = json!("Changed contract");
    save(&mut w, "spec:same", None, data);
    assert_eq!(
        objects::verification(&w.state, "measure:same"),
        "unverified"
    );
    assert_eq!(
        objects::verification(&w.state, "work:implement"),
        "unverified"
    );
    assert_eq!(
        tasks::status(&w.state.task_process["work:implement"]),
        "done"
    );
    assert_eq!(
        serde_json::to_value(&w.state.objects["spec:same"].revisions[..old.len()]).unwrap(),
        serde_json::to_value(old).unwrap()
    );
}
#[test]
fn nested_specifications_select_the_nearest_context_and_reject_evidence_injection() {
    let mut w = setup_custom();
    save(
        &mut w,
        "spec:outer",
        None,
        json!({"title":"Outer","body":"Outer contract","owner":"owner"}),
    );
    save(
        &mut w,
        "epic:inner",
        Some("spec:outer"),
        json!({"title":"Inner","body":"Inner contract","owner":"owner"}),
    );
    save(
        &mut w,
        "section:middle",
        Some("epic:inner"),
        json!({"title":"Middle","owner":"owner"}),
    );
    save(
        &mut w,
        "measure:check",
        Some("section:middle"),
        json!({"title":"Check","owner":"owner"}),
    );
    assert_eq!(
        objects::verification_context(&w.state, "measure:check")
            .unwrap()
            .0,
        "epic:inner"
    );
    assert!(
        w.state.requirements["spec:outer"]
            .revisions
            .last()
            .unwrap()
            .acs
            .is_empty()
    );
    let n = w.state.objects["section:middle"].current().revision;
    w.no("agent",json!({"op":"save_object","kind":"section","id":"middle","revision":n,"parent":"spec:outer","data":{"title":"Middle","owner":"owner"},"reason":"Move evidence across contracts"}));
    ready(&mut w, "epic:inner", &["measure:check"]);
    run(
        &mut w,
        "epic:inner",
        "inner-pass",
        &[("measure:check", "pass")],
    );
    assert_eq!(objects::verification(&w.state, "epic:inner"), "pass");
    assert_eq!(objects::verification(&w.state, "spec:outer"), "unverified");
    let n = w.state.objects["measure:check"].current().revision;
    w.no("agent",json!({"op":"save_object","kind":"measure","id":"check","revision":n,"parent":"section:middle","data":{"title":"Forged","owner":"owner","assessment":{"ready":true}},"reason":"Inject evidence"}));
    let mut replay = Workspace::default();
    for (position, value) in setup().into_iter().enumerate() {
        let cmd: Command = serde_json::from_value(value).unwrap();
        commands::apply_replay(
            &mut replay,
            "owner",
            &cmd,
            position as u64 + 1,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
    }
    assert_eq!(
        serde_json::to_value(replay.objects).unwrap(),
        serde_json::to_value(World::new().state.objects).unwrap()
    );
}

#[test]
fn custom_workflow_ids_do_not_collide_with_legacy_verification_ids() {
    let mut w = World::new();
    objects::migrate_notes(&mut w.state, "owner", w.position + 1, "now").unwrap();
    objects::migrate_schema(&mut w.state, "owner", w.position + 1, "now").unwrap();
    let mut types = objects::schema(&w.state).types;
    let custom: std::collections::BTreeMap<String, objects::TreeType> =
        serde_json::from_value(json!({
            "spec":{"title":"Spec","role":"requirement","root":true,"parents":["*"]},
            "measure":{"title":"Measure","role":"ac","root":false,"parents":["*"]}
        }))
        .unwrap();
    types.extend(custom);
    let n = objects::schema(&w.state).revision;
    w.ok("agent",json!({"op":"set_tree_schema","revision":n,"types":types,"reason":"Add custom verification types"}));
    let original = serde_json::to_value(&w.state.requirements["r"]).unwrap();
    save(
        &mut w,
        "spec:r",
        None,
        json!({"title":"Independent","body":"Independent contract","owner":"owner"}),
    );
    save(
        &mut w,
        "measure:ios",
        Some("spec:r"),
        json!({"title":"Independent iOS result","owner":"owner"}),
    );
    assert_eq!(
        serde_json::to_value(&w.state.requirements["r"]).unwrap(),
        original
    );
    assert_eq!(
        objects::criterion_context(&w.state, "ac:ios").unwrap().0,
        "r"
    );
    assert_eq!(
        objects::criterion_context(&w.state, "measure:ios")
            .unwrap()
            .0,
        "spec:r"
    );
}

#[test]
fn old_type_names_do_not_select_verification_when_the_schema_assigns_other_roles() {
    let mut w = World {
        state: Workspace::default(),
        position: 0,
    };
    w.ok(
        "owner",
        json!({"op":"workspace","title":"Names do not select behavior"}),
    );
    w.ok("owner", json!({"op":"member","id":"agent","role":"agent"}));
    w.ok(
        "agent",
        json!({"op":"set_tree_schema","revision":0,"types":{
        "requirement":{"title":"Group","role":"group","root":true,"parents":[]},
        "ac":{"title":"Input","role":"document","root":true,"parents":["*"],"inbox":true}
    },"reason":"Configure behavior independently of type names"}),
    );
    objects::migrate_schema(&mut w.state, "owner", w.position + 1, "now").unwrap();
    save(
        &mut w,
        "requirement:plain",
        None,
        json!({"title":"Ordinary group","owner":"owner"}),
    );
    save(
        &mut w,
        "ac:source",
        Some("requirement:plain"),
        json!({"title":"Input text","owner":"owner","body":"A new fact"}),
    );
    assert!(w.state.requirements.is_empty());
    assert_eq!(
        objects::process(&w.state, "ac:source")["status"],
        "unprocessed"
    );
    assert_eq!(
        objects::verification(&w.state, "requirement:plain"),
        "unverified"
    );
}
