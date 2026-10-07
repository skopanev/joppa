mod common;
use common::*;
use joppa::{objects, tasks};
use serde_json::{Value, json};

fn fixture() -> World {
    let mut w = World::new();
    for ac in ["ios", "key"] {
        w.ok("agent", assess(ac, true));
    }
    let mut types = objects::defaults();
    let mut job = types["task"].clone();
    job.root = true;
    job.parents = vec!["*".into()];
    job.parent_gate = None;
    types.insert("job".into(), job);
    let mut section = types["capability"].clone();
    section.root = true;
    section.parents = vec!["*".into()];
    types.insert("section".into(), section);
    w.ok("agent", json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Use an arbitrary Task type and parent"}));
    tasks::migrate(&mut w.state, "owner", w.position + 1, "test").unwrap();
    for (id, parent) in [
        ("one", Some("ac:ios")),
        ("two", Some("ac:ios")),
        ("other", Some("ac:key")),
        ("whole", Some("requirement:r")),
        ("root", None),
    ] {
        w.ok("agent", json!({"op":"save_object","kind":"job","id":id,"revision":0,"parent":parent,"data":{"title":id,"owner":"owner","contract":"Implement the exact fixture behavior","purpose":"implementation","projects":["app"]},"reason":"Create explicit fixture work"}));
    }
    w
}
fn bound_plan(w: &World, run: &str, addresses: &[&str]) -> Value {
    let mut p = plan(run);
    p["checks"][0]["tasks"] = json!(addresses.iter().map(|key| json!({"address":key,"origin_revision":w.state.task_process[*key].origin_revision})).collect::<Vec<_>>());
    p
}
fn finish(w: &mut World, run: &str, ios: Option<&str>, key: &str) {
    if let Some(outcome) = ios {
        w.ok(
            "runner",
            result(run, "[ios] paid access", outcome, vec!["access retained"]),
        );
    }
    w.ok(
        "owner",
        result(run, "key-permissions", key, vec!["read", "verify"]),
    );
    w.ok(
        "runner",
        json!({"op":"finish","req":"r","revision":1,"run":run}),
    );
}
fn verdict(w: &World, key: &str) -> &'static str {
    objects::verification(&w.state, key)
}
fn action(w: &mut World, key: &str, op: &str, extra: Value) {
    let mut command = json!({"op":op,"id":key,"revision":w.state.objects[key].current().revision});
    for (key, value) in extra.as_object().unwrap() {
        command[key] = value.clone();
    }
    w.ok("agent", command);
}

// [task-check-evidence-binding]
#[test]
fn bindings_reject_stale_unknown_duplicate_and_foreign_context_atomically() {
    let mut w = fixture();
    let mut stale = bound_plan(&w, "stale", &["job:one"]);
    stale["checks"][0]["tasks"][0]["origin_revision"] = json!(99);
    w.no("runner", stale);
    let mut unknown = bound_plan(&w, "unknown", &["job:one"]);
    unknown["checks"][0]["tasks"][0]["address"] = json!("job:absent");
    w.no("runner", unknown);
    w.no(
        "runner",
        bound_plan(&w, "duplicate", &["job:one", "job:one"]),
    );
    w.no("runner", bound_plan(&w, "criterion", &["job:other"]));
    w.no("runner", bound_plan(&w, "context", &["job:root"]));
    assert!(w.state.requirements["r"].revisions[0].runs.is_empty());
    w.ok(
        "runner",
        bound_plan(&w, "valid", &["job:one", "job:two", "job:whole"]),
    );
}

// [task-check-evidence-verdict]
#[test]
fn shared_checks_verify_tasks_without_completing_or_confirming_work() {
    let mut w = fixture();
    w.ok(
        "runner",
        bound_plan(&w, "shared", &["job:one", "job:two", "job:whole"]),
    );
    finish(&mut w, "shared", Some("pass"), "fail");
    for key in ["job:one", "job:two", "job:whole"] {
        assert_eq!(verdict(&w, key), "pass");
        assert_eq!(tasks::process(&w.state, key).unwrap()["status"], "open");
    }
    assert_eq!(verdict(&w, "job:other"), "unverified");
    assert_eq!(verdict(&w, "ac:ios"), "unverified");
    assert!(!w.state.requirements["r"].revisions[0].confirmed);
}

// [task-check-evidence-verdict]
#[test]
fn missing_skipped_failed_and_unfinished_results_do_not_reuse_a_pass() {
    for (name, outcome, expected) in [
        ("missing", None, "unverified"),
        ("skip", Some("skip"), "unverified"),
        ("fail", Some("fail"), "fail"),
    ] {
        let mut w = fixture();
        w.ok("runner", bound_plan(&w, "old", &["job:one"]));
        finish(&mut w, "old", Some("pass"), "pass");
        w.ok("runner", bound_plan(&w, name, &["job:one"]));
        assert_eq!(verdict(&w, "job:one"), "unverified");
        finish(&mut w, name, outcome, "pass");
        assert_eq!(verdict(&w, "job:one"), expected);
    }
}

// [task-check-evidence-invalidation]
#[test]
fn lifecycle_preserves_contract_binding_and_late_runs_cannot_replace_current_evidence() {
    let mut w = fixture();
    let origin = w.state.task_process["job:one"].origin_revision;
    w.ok("runner", bound_plan(&w, "late", &["job:one"]));
    w.ok("runner", bound_plan(&w, "current", &["job:one"]));
    action(&mut w, "job:one", "claim_task", json!({}));
    action(
        &mut w,
        "job:one",
        "release_task",
        json!({"reason":"Return fixture work"}),
    );
    action(&mut w, "job:one", "claim_task", json!({}));
    action(
        &mut w,
        "job:one",
        "complete_task",
        json!({"evidence":"Fixture work delivered"}),
    );
    assert_eq!(w.state.task_process["job:one"].origin_revision, origin);
    finish(&mut w, "current", Some("fail"), "pass");
    finish(&mut w, "late", Some("pass"), "pass");
    assert_eq!(verdict(&w, "job:one"), "fail");
    assert_eq!(
        objects::signal(&w.state, "job:one")["label"],
        "Work complete · Checks failed"
    );
    assert!(w.state.task_process["job:one"].done.is_some());
}

// [task-check-evidence-coverage]
#[test]
fn a_plan_cannot_remove_required_task_bindings_to_hide_failure() {
    let mut w = fixture();
    w.ok("runner", bound_plan(&w, "failed", &["job:one", "job:two"]));
    finish(&mut w, "failed", Some("fail"), "pass");
    w.no("runner", bound_plan(&w, "drop", &["job:two"]));
    assert_eq!(verdict(&w, "job:one"), "fail");
}

// [task-check-evidence-coverage]
#[test]
fn changed_task_contract_records_coverage_review_before_a_new_pass() {
    let mut w = fixture();
    w.ok("runner", bound_plan(&w, "old", &["job:one"]));
    finish(&mut w, "old", Some("fail"), "pass");
    let old = w.state.objects["job:one"].current().clone();
    let mut data = old.data.clone();
    data["contract"] = json!("Implement the revised fixture behavior");
    w.ok("agent",json!({"op":"save_object","kind":"job","id":"one","revision":old.revision,"parent":old.parent,"data":data,"reason":"Change the technical contract"}));
    assert_eq!(verdict(&w, "job:one"), "unverified");
    let mut p = bound_plan(&w, "new", &["job:one"]);
    p["enforce_coverage"] = json!(true);
    w.ok("runner", p);
    finish(&mut w, "new", Some("pass"), "pass");
    assert_eq!(verdict(&w, "job:one"), "unverified");
    w.ok("owner",json!({"op":"review_coverage","req":"r","revision":1,"run":"new","reason":"Review the changed Task contract binding"}));
    assert_eq!(verdict(&w, "job:one"), "pass");
    assert_eq!(
        w.state.requirements["r"].revisions[0].runs["old"].results["[ios] paid access"].outcome,
        "fail"
    );
}

// [task-check-evidence-invalidation]
#[test]
fn reviewing_a_changed_group_does_not_revive_its_old_test_evidence() {
    let mut w = fixture();
    w.ok("agent",json!({"op":"save_object","kind":"section","id":"nested","revision":0,"parent":"ac:ios","data":{"title":"Source group","owner":"owner"},"reason":"Add an intermediate source"}));
    let task = w.state.objects["job:one"].current().clone();
    w.ok("agent",json!({"op":"save_object","kind":"job","id":"one","revision":task.revision,"parent":"section:nested","data":task.data,"reason":"Place work in the source group"}));
    w.ok("owner",json!({"op":"review_task","id":"job:one","revision":w.state.objects["job:one"].current().revision,"reason":"Review the current source group"}));
    w.ok("runner", bound_plan(&w, "old-group", &["job:one"]));
    finish(&mut w, "old-group", Some("pass"), "pass");
    assert_eq!(verdict(&w, "job:one"), "pass");
    let group = w.state.objects["section:nested"].current().clone();
    w.ok("agent",json!({"op":"save_object","kind":"section","id":"nested","revision":group.revision,"parent":group.parent,"data":{"title":"Changed source group","owner":"owner"},"reason":"Change the intermediate source"}));
    assert_eq!(verdict(&w, "job:one"), "unverified");
    w.ok("owner",json!({"op":"review_task","id":"job:one","revision":w.state.objects["job:one"].current().revision,"reason":"Review the changed source group"}));
    assert_eq!(verdict(&w, "job:one"), "unverified");
    w.ok("runner", bound_plan(&w, "new-group", &["job:one"]));
    finish(&mut w, "new-group", Some("pass"), "pass");
    assert_eq!(verdict(&w, "job:one"), "pass");
}
