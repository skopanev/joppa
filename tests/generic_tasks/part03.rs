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
