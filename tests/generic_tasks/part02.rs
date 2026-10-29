// Part 2 of tests/generic_tasks.rs, included there, so the test names stay unchanged.

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
    action(
        &mut w,
        "owner",
        "task:one",
        "retire_task",
        json!({"reason":"The work is no longer required"}),
    );
    let signal = objects::signal(&w.state, "task:one");
    assert_eq!(
        (signal["tone"].as_str(), signal["label"].as_str()),
        (Some("grey"), Some("Retired"))
    );
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

// edit-makes-revision-ac3
#[test]
fn an_identical_save_keeps_the_workspace_and_a_change_adds_one_revision() {
    let mut w = configured();
    save(&mut w, "work:source", None, contract("Source task"));
    w.ok("agent",json!({"op":"save_object","kind":"task","id":"dependent","revision":0,"parent":"spec:root","data":contract("Dependent task"),"links":[{"target":"work:source","relation":"depends_on","revision":1,"blocking":true}],"reason":"Depend on the source"}));
    let before = format!("{:?}", w.state);
    save(&mut w, "work:source", None, contract("Source task"));
    assert_eq!(format!("{:?}", w.state), before);
    let mut changed = contract("Source task");
    changed["contract"] = json!("Implement the changed document behavior");
    save(&mut w, "work:source", None, changed);
    assert_eq!(w.state.objects["work:source"].revisions.len(), 2);
    assert!(!objects::review_links(&w.state, "task:dependent").is_empty());

    let linked = |revision: u32, links: Value| json!({"op":"save_object","kind":"spec","id":"child","revision":revision,"parent":"spec:root","data":document("Nested specification"),"links":links,"reason":"Relate the specification"});
    w.ok("agent", linked(1, json!([])));
    assert_eq!(w.state.objects["spec:child"].revisions.len(), 1);
    w.ok(
        "agent",
        linked(1, json!([{"target":"spec:root","relation":"related"}])),
    );
    assert_eq!(w.state.objects["spec:child"].revisions.len(), 2);

    // A stored identical save still adds its historical revision on replay.
    let cmd: Command = serde_json::from_value(linked(
        2,
        json!([{"target":"spec:root","relation":"related"}]),
    ))
    .unwrap();
    commands::apply_replay(&mut w.state, "agent", &cmd, w.position + 1, "later").unwrap();
    assert_eq!(w.state.objects["spec:child"].revisions.len(), 3);
}

// [ac:pins-ignore-process-events-ac1]
#[test]
fn claim_and_completion_keep_a_dependency_pin_current() {
    let mut w = World::new();
    w.ready();
    tasks::migrate(&mut w.state, "owner", w.position + 1, "later").unwrap();
    let pinned = |w: &mut World, source: &str, dependent: &str| {
        save(w, source, Some("ac:ios"), contract("Source task"));
        let pin = w.state.objects[source].current().revision;
        let (kind, id) = dependent.split_once(':').unwrap();
        w.ok("agent",json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":"ac:ios","data":contract("Dependent task"),"links":[{"target":source,"relation":"depends_on","revision":pin,"blocking":true}],"reason":"Depend on the source"}));
        pin
    };
    let claimable = |w: &World, key: &str| {
        tasks::claimable(&w.state, "agent", key, &w.state.task_process[key]).is_ok()
    };
    let pin = pinned(&mut w, "task:a", "task:b");
    for key in ["task:a", "task:b"] {
        assert!(tasks::readiness_blocker(&w.state, key).is_none());
    }
    assert!(claimable(&w, "task:a"));
    assert!(!claimable(&w, "task:b"));
    action(&mut w, "agent", "task:a", "claim_task", json!({}));
    assert!(objects::review_links(&w.state, "task:b").is_empty());
    action(
        &mut w,
        "agent",
        "task:a",
        "complete_task",
        json!({"evidence":"The source is complete"}),
    );
    assert_eq!(objects::process(&w.state, "task:a")["status"], "done");
    assert!(w.state.objects["task:a"].current().revision > pin);
    assert!(objects::review_links(&w.state, "task:b").is_empty());
    assert!(objects::work_ready(&w.state, "task:b"));
    assert!(claimable(&w, "task:b"));
    assert_eq!(
        w.state.objects["task:b"].current().links[0].revision,
        Some(pin)
    );

    pinned(&mut w, "task:c", "task:d");
    let revision = w.state.objects["task:c"].current().revision;
    w.legacy("agent",json!({"op":"amend_task","id":"task:c","revision":revision,"title":"Changed source task","contract":"Implement the specified document behavior","projects":[],"reason":"Change the source"}));
    assert!(!objects::review_links(&w.state, "task:d").is_empty());
    assert!(!claimable(&w, "task:d"));
}

#[test]
fn migration_preserves_old_task_history_and_uses_one_process() {
    let mut w = World::new();
    w.ready();
    w.legacy("agent",json!({"op":"task","req":"r","revision":1,"id":"old","title":"Existing task","contract":"Implement the iOS flow","acs":["ios"],"projects":["app"],"purpose":"implementation"}));
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
