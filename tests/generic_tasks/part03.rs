// Part 3 of tests/generic_tasks.rs, included there, so the test names stay unchanged.

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
    w.no("legal",json!({"op":"complete_task","commit":"c0ffee0","id":"task:decision","revision":revision,"evidence":"A free text decision"}));
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
