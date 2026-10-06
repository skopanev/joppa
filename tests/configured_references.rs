mod common;
use common::*;
use joppa::objects::{self, ReferenceRule, TreeType};
use serde_json::{Value, json};

fn save(w: &mut World, key: &str, parent: Option<&str>, data: Value) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok("agent", json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Set configured references"}));
}
fn migrate(w: &mut World) {
    objects::migrate_notes(&mut w.state, "owner", w.position + 1, "now").unwrap();
    joppa::tasks::migrate(&mut w.state, "owner", w.position + 2, "now").unwrap();
    objects::migrate_schema(&mut w.state, "owner", w.position + 3, "now").unwrap();
    w.position += 3;
}
fn set_schema(w: &mut World, types: std::collections::BTreeMap<String, TreeType>) {
    w.ok("agent", json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Configure reference fields"}));
}
fn rule(target: &str, multiple: bool, required: bool) -> ReferenceRule {
    ReferenceRule {
        target: target.into(),
        title: "Affects".into(),
        multiple,
        required,
        verification_subject: false,
    }
}

#[test]
fn custom_task_references_use_the_schema_without_consumers_or_projects() {
    let mut w = World::new();
    migrate(&mut w);
    let mut types = objects::schema(&w.state).types;
    let task = types.get_mut("task").unwrap();
    task.parents = vec!["domain".into()];
    task.parent_gate = None;
    task.parent_decision = false;
    task.references = [("affected".into(), rule("domain", true, true))].into();
    set_schema(&mut w, types);
    let data = json!({"title":"Implement access","owner":"owner","contract":"Implement the agreed access behavior","purpose":"implementation","affected":["d"]});
    save(&mut w, "task:custom", Some("domain:d"), data.clone());
    assert!(objects::referenced(&w.state, "task:custom", "domain:d"));
    assert_eq!(w.state.objects["task:custom"].current().data, data);
    assert!(w.state.task_process["task:custom"].projects.is_empty());
    let revision = w.state.objects["task:custom"].current().revision;
    for bad in [
        json!([]),
        json!(["d", "domain:d"]),
        json!(["consumer:customer"]),
        json!("d"),
    ] {
        let mut invalid = data.clone();
        invalid["affected"] = bad;
        w.no("agent",json!({"op":"save_object","kind":"task","id":"custom","revision":revision,"parent":"domain:d","data":invalid,"reason":"Reject invalid references"}));
    }
    w.ok(
        "agent",
        json!({"op":"claim_task","id":"task:custom","revision":revision}),
    );
    w.ok("agent",json!({"op":"complete_task","id":"task:custom","revision":revision+1,"evidence":"Implemented the contract"}));
    assert!(w.state.task_process["task:custom"].done.is_some());
}

#[test]
fn native_requirement_keeps_custom_references_through_process_commands() {
    let mut w = World::new();
    migrate(&mut w);
    let mut types = objects::schema(&w.state).types;
    types.get_mut("requirement").unwrap().references =
        [("audiences".into(), rule("domain", true, false))].into();
    set_schema(&mut w, types);
    save(
        &mut w,
        "requirement:r",
        Some("capability:c"),
        json!({"title":"Access","body":"Access follows the domain contract","owner":"owner","audiences":["d"]}),
    );
    let number = w.state.requirements["r"].revisions.last().unwrap().number;
    w.ok("owner", json!({"op":"confirm","req":"r","revision":number}));
    assert_eq!(
        w.state.objects["requirement:r"].current().data["audiences"],
        json!(["d"])
    );
    assert!(objects::referenced(&w.state, "ac:ios", "domain:d"));
    assert!(!objects::referenced(
        &w.state,
        "ac:ios",
        "consumer:customer"
    ));
    let mut types = objects::schema(&w.state).types;
    types.get_mut("requirement").unwrap().references.clear();
    set_schema(&mut w, types);
    assert!(!objects::referenced(&w.state, "ac:ios", "domain:d"));
    save(
        &mut w,
        "requirement:new",
        Some("capability:c"),
        json!({"title":"New contract","body":"Read the current contract","owner":"owner"}),
    );
    assert!(
        w.state.requirements["new"]
            .revisions
            .last()
            .unwrap()
            .consumers
            .is_empty()
    );
}

#[test]
fn schema_migration_preserves_document_history_and_runs_once() {
    let mut w = World::new();
    objects::migrate_notes(&mut w.state, "owner", w.position + 1, "now").unwrap();
    joppa::tasks::migrate(&mut w.state, "owner", w.position + 2, "now").unwrap();
    w.position += 2;
    let before = serde_json::to_value(&w.state.objects).unwrap();
    let revision = objects::schema(&w.state).revision;
    objects::migrate_schema(&mut w.state, "owner", w.position + 1, "now").unwrap();
    w.position += 1;
    assert_eq!(before, serde_json::to_value(&w.state.objects).unwrap());
    assert_eq!(objects::schema(&w.state).revision, revision + 1);
    assert!(objects::schema(&w.state).types["requirement"].references["consumers"].required);
    assert!(
        objects::schema(&w.state)
            .types
            .values()
            .all(|t| t.index.is_none())
    );
    objects::migrate_schema(&mut w.state, "owner", w.position + 1, "now").unwrap();
    w.position += 1;
    assert_eq!(objects::schema(&w.state).revision, revision + 1);
    assert!(
        objects::legacy_defaults()
            .values()
            .all(|t| t.references.is_empty() && t.color.is_none() && !t.parent_decision)
    );
}

#[test]
fn wildcard_references_allow_multiple_types_and_require_full_addresses() {
    let mut w = World::new();
    migrate(&mut w);
    let mut types = objects::schema(&w.state).types;
    types.get_mut("note").unwrap().references = [("context".into(), rule("*", true, false))].into();
    set_schema(&mut w, types);
    save(
        &mut w,
        "note:input",
        None,
        json!({"title":"Incoming evidence","body":"Check both contexts","owner":"owner","context":["domain:d","consumer:customer"]}),
    );
    assert_eq!(
        objects::reference_targets(
            &w.state,
            "note",
            &w.state.objects["note:input"].current().data
        )
        .unwrap()
        .len(),
        2
    );
    assert!(objects::reference_targets(&w.state, "note", &json!({"context":["d"]})).is_err());
}

#[test]
fn inherited_subjects_constrain_claims_and_check_plans() {
    let mut w = World::new();
    migrate(&mut w);
    let mut types = objects::schema(&w.state).types;
    let mut subject = rule("project", true, false);
    subject.verification_subject = true;
    types
        .get_mut("requirement")
        .unwrap()
        .references
        .insert("systems".into(), subject);
    set_schema(&mut w, types);
    let mut data = w.state.objects["requirement:r"].current().data.clone();
    data["systems"] = json!(["backend"]);
    save(&mut w, "requirement:r", Some("capability:c"), data);
    let number = w.state.requirements["r"].revisions.last().unwrap().number;
    w.ok("owner", json!({"op":"confirm","req":"r","revision":number}));
    w.ok("agent",json!({"op":"assess","req":"r","revision":number,"ac":"ios","assessment":{"ready":true,"reason":"Contract is clear"}}));
    save(
        &mut w,
        "task:inherited",
        Some("ac:ios"),
        json!({"title":"Implement access","owner":"owner","contract":"Implement the approved contract","purpose":"implementation","projects":[]}),
    );
    assert_eq!(
        joppa::tasks::subjects(&w.state, "task:inherited").unwrap(),
        vec!["backend"]
    );
    w.ok("owner",json!({"op":"define_verification_scope","req":"r","revision":number,"subject_keys":["app"],"scope":"Access","reason":"Check the declared subject"}));
    let task_revision = w.state.objects["task:inherited"].current().revision;
    w.no(
        "agent",
        json!({"op":"claim_task","id":"task:inherited","revision":task_revision}),
    );
    w.no("runner",json!({"op":"plan","req":"r","revision":number,"run":"missing-inherited","subject":{"app":"build1"},"scope":"Access","discovery_complete":true,"checks":[{"id":"access","ac":"ios","method":"automated","expected":["Access retained"]},{"id":"key","ac":"key","method":"manual","expected":["Correct key"]}]}));
}

#[test]
fn a_default_reference_field_can_select_one_target_of_another_type() {
    let mut w = World::new();
    migrate(&mut w);
    let mut types = objects::schema(&w.state).types;
    types.get_mut("requirement").unwrap().references =
        [("consumers".into(), rule("domain", false, true))].into();
    set_schema(&mut w, types);
    save(
        &mut w,
        "requirement:single",
        Some("capability:c"),
        json!({"title":"Single target","body":"Follow the domain contract","owner":"owner","consumers":"domain:d"}),
    );
    assert_eq!(
        w.state.objects["requirement:single"].current().data["consumers"],
        "domain:d"
    );
    assert!(objects::referenced(
        &w.state,
        "requirement:single",
        "domain:d"
    ));
    let number = w.state.requirements["single"]
        .revisions
        .last()
        .unwrap()
        .number;
    w.ok(
        "owner",
        json!({"op":"confirm","req":"single","revision":number}),
    );
    assert_eq!(
        w.state.objects["requirement:single"].current().data["consumers"],
        "domain:d"
    );
    save(
        &mut w,
        "ac:single-criterion",
        Some("requirement:single"),
        json!({"title":"Apply the domain contract","owner":"owner"}),
    );
    assert_eq!(
        w.state.objects["requirement:single"].current().data["consumers"],
        "domain:d"
    );
}
