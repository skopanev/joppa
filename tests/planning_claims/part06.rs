// Part 6 of tests/planning_claims.rs, included there, so the test names stay unchanged.

// failed-task-blocks-ac5
#[test]
fn planner_unblocks_a_task_and_its_dependent_pin_stays_current() {
    let mut w = blocked_plan();
    // U under B depends on T with a blocking link pinned to the current T revision.
    let pin = w.state.objects["work:t"].current().revision;
    w.ok("owner", json!({"op":"save_object","kind":"work","id":"u","revision":0,"parent":"criterion:b","data":{"title":"work:u","owner":"owner","body":"Implement this criterion","purpose":"implementation"},"links":[{"target":"work:t","relation":"depends_on","revision":pin,"blocking":true}],"reason":"Plan dependent work"}));
    // The revision check comes first.
    let mut stale = unblock(&w, "work:t");
    stale["revision"] = json!(pin - 1);
    stale["reason"] = json!(" ");
    refuse(
        &mut w,
        "planner",
        stale,
        "stale Task revision; read the Task again",
    );
    for field in ["reason", "evidence"] {
        let mut empty = unblock(&w, "work:t");
        empty[field] = json!(" ");
        refuse(&mut w, "planner", empty, "text must not be empty");
    }
    for (field, value) in [("blocked", Some(json!(null))), ("evidence", None)] {
        let mut wrong = unblock(&w, "work:t");
        match value {
            Some(value) => wrong[field] = value,
            None => {
                wrong.as_object_mut().unwrap().remove(field);
            }
        }
        let before = serde_json::to_value(&w.state).unwrap();
        let position = w.position;
        assert!(w.apply("planner", wrong).is_err());
        assert_eq!(
            (w.position, serde_json::to_value(&w.state).unwrap()),
            (position, before)
        );
    }
    // A member without a claim and the former holder have no authority.
    for actor in ["other", "worker"] {
        let cmd = unblock(&w, "work:t");
        refuse(&mut w, actor, cmd, PLANNER_OR_OWNER);
    }

    unblock_and_replay(&mut w, "planner");
    let t = &w.state.task_process["work:t"];
    assert!(tasks::claimable(&w.state, "other", "work:t", t).is_ok());
    assert_eq!(
        objects::process(&w.state, "work:u")["review_required"],
        false
    );
    let cmd = unblock(&w, "work:t");
    refuse(&mut w, "planner", cmd, "Task is not blocked");
    finish_and_replay(&mut w);
    assert_eq!(
        objects::process(&w.state, "work:u")["review_required"],
        false
    );
}

// failed-task-blocks-ac5
#[test]
fn owner_unblocks_without_a_claim_and_a_retired_task_cannot_be_unblocked() {
    let mut w = held();
    w.ok("worker", block(&w, "work:t"));
    unblock_and_replay(&mut w, "owner");
    let cmd = unblock(&w, "work:t");
    refuse(&mut w, "owner", cmd, "Task is not blocked");

    let mut w = blocked_plan();
    let cmd = task_command(
        &w,
        "retire_task",
        "work:t",
        json!({"reason":"The planned work replaces this Task"}),
    );
    w.ok("planner", cmd);
    let cmd = unblock(&w, "work:t");
    refuse(&mut w, "owner", cmd, "retired Task cannot be unblocked");
}
