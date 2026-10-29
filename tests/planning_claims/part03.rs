// Part 3 of tests/planning_claims.rs, included there, so the test names stay unchanged.

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
        json!({"op":"enroll_service","id":"runner","role":"runner"}),
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
    // Recorded status is checked by the transactional store, not by legacy run metadata.
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

const BLOCKED: &str = "Task is blocked for planning";

fn task_command(w: &World, op: &str, key: &str, extra: Value) -> Value {
    let mut cmd = json!({"op":op,"id":key,"revision":w.state.objects[key].current().revision});
    for (field, value) in extra.as_object().unwrap() {
        cmd[field] = value.clone();
    }
    cmd
}

fn block(w: &World, key: &str) -> Value {
    task_command(
        w,
        "block_task",
        key,
        json!({"reason":"Verification failed","evidence":"test x failed on abc123"}),
    )
}

/// Fixture 1: A is ready with valid dependencies, no verification and no planning claim.
/// L (`worker`) holds T at revision 4.
fn held() -> World {
    let mut w = configured();
    work(&mut w, "planner", "work:t");
    for (op, extra) in [
        ("claim_task", json!({})),
        ("release_task", json!({"reason":"Return the Task"})),
        ("claim_task", json!({})),
    ] {
        let cmd = task_command(&w, op, "work:t", extra);
        w.ok("worker", cmd);
    }
    assert_eq!(w.state.objects["work:t"].current().revision, 4);
    let a = objects::process(&w.state, "criterion:a");
    assert_eq!(a["assessment"]["state"], "ready");
    assert!(a["planning_claim"].is_null() && a["verification"] != "pass");
    assert!(objects::work_ready(&w.state, "criterion:a"));
    w
}

// failed-task-blocks-ac1
#[test]
fn holder_blocks_a_held_task_and_refusals_keep_state_and_position() {
    let mut w = held();
    let unblocked = objects::process(&w.state, "work:t");
    assert!(unblocked.get("blocked").is_some_and(Value::is_null));
    let mut stale = block(&w, "work:t");
    stale["revision"] = json!(3);
    refuse(
        &mut w,
        "worker",
        stale,
        "stale Task revision; read the Task again",
    );
    let cmd = block(&w, "work:t");
    refuse(&mut w, "other", cmd, "Task holder required");
    for field in ["reason", "evidence"] {
        let mut empty = block(&w, "work:t");
        empty[field] = json!(" ");
        refuse(&mut w, "worker", empty, "text must not be empty");
    }
    let mut unknown = block(&w, "work:t");
    unknown["blocked"] = json!(true);
    let before = serde_json::to_value(&w.state).unwrap();
    let position = w.position;
    assert!(w.apply("worker", unknown).is_err());
    assert_eq!(
        (w.position, serde_json::to_value(&w.state).unwrap()),
        (position, before)
    );

    let before = w.state.clone();
    let command = block(&w, "work:t");
    w.ok("worker", command.clone());
    let p = objects::process(&w.state, "work:t");
    assert_eq!(p["status"], "open");
    assert!(p["assignee"].is_null() && p.get("claim").is_none_or(Value::is_null));
    assert_eq!(
        p["blocked"],
        json!({"actor":"worker","reason":"Verification failed","evidence":"test x failed on abc123","position":w.position})
    );
    assert_eq!(w.state.objects["work:t"].current().position, w.position);
    assert_eq!(p["revision"], 5);

    // Replay of the same event gives the same block and holder state.
    let mut replayed = before;
    let cmd: Command = serde_json::from_value(command).unwrap();
    commands::apply_replay(
        &mut replayed,
        "worker",
        &cmd,
        w.position,
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    assert_eq!(objects::process(&replayed, "work:t"), p);
    assert_eq!(
        serde_json::to_value(&replayed).unwrap(),
        serde_json::to_value(&w.state).unwrap()
    );

    // The former holder lost its authority with the claim.
    let cmd = block(&w, "work:t");
    refuse(&mut w, "worker", cmd, "Task is not claimed unfinished work");
    let cmd = block(&w, "work:t");
    refuse(&mut w, "owner", cmd, "Task is already blocked");
}
