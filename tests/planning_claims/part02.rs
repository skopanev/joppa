// Part 2 of tests/planning_claims.rs, included there, so the test names stay unchanged.

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
    save(
        &mut w,
        "planner",
        "work:old",
        Some("criterion:a"),
        json!({"title":"work:old","owner":"owner","contract":"Implement the changed criterion","purpose":"implementation"}),
    );
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
