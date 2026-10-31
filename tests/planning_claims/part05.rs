// Part 5 of tests/planning_claims.rs, included there, so the test names stay unchanged.

// failed-task-blocks-ac3
#[test]
fn blocked_contract_ignores_link_order_and_starts_at_the_block() {
    let mut w = held();
    for id in ["work:x", "work:y"] {
        work(&mut w, "planner", id);
    }
    let links = json!([dependency("work:x"), dependency("work:y")]);
    save_t(&mut w, "owner", t_document(), links);
    w.ok("worker", block(&w, "work:t"));
    claim(&mut w, "planner", "criterion:a");
    let reordered = json!([dependency("work:y"), dependency("work:x")]);
    save_t(&mut w, "planner", t_document(), reordered);
    refuse(&mut w, "planner", finish(), NEEDS_CONTRACT);
    save_t(
        &mut w,
        "planner",
        t_document(),
        json!([dependency("work:x")]),
    );
    finish_and_replay(&mut w);
    assert!(objects::process(&w.state, "work:t")["blocked"].is_null());

    // A contract change after the block and before the claim also counts.
    let mut w = held();
    w.ok("worker", block(&w, "work:t"));
    let mut data = t_document();
    data["body"] = json!("Implement the criterion with a new fix");
    save_t(&mut w, "owner", data, json!([]));
    claim(&mut w, "planner", "criterion:a");
    refuse(
        &mut w,
        "planner",
        finish(),
        "planning produced no Task changes",
    );
    work(&mut w, "planner", "work:z");
    finish_and_replay(&mut w);
    assert!(objects::process(&w.state, "work:t")["blocked"].is_null());
}

// failed-task-blocks-ac3
#[test]
fn finish_checks_every_blocked_task_and_keeps_blocks_on_other_refusals() {
    let mut w = held();
    work(&mut w, "planner", "work:u");
    w.ok("other", task_command(&w, "claim_task", "work:u", json!({})));
    w.ok("owner", block(&w, "work:u"));
    w.ok("worker", block(&w, "work:t"));
    claim(&mut w, "planner", "criterion:a");
    let mut data = t_document();
    data["body"] = json!("Implement the criterion with a new fix");
    save_t(&mut w, "planner", data, json!([]));
    refuse(&mut w, "planner", finish(), NEEDS_CONTRACT);
    // A failed dependency check keeps both blocks.
    w.state
        .objects
        .get_mut("work:t")
        .unwrap()
        .revisions
        .last_mut()
        .unwrap()
        .links = vec![objects::ObjectLink {
        target: "work:missing".into(),
        relation: "depends_on".into(),
        revision: Some(1),
        blocking: true,
    }];
    let cmd = task_command(
        &w,
        "retire_task",
        "work:u",
        json!({"reason":"The work is no longer required"}),
    );
    w.ok("planner", cmd);
    refuse(
        &mut w,
        "planner",
        finish(),
        "planning dependency target is missing",
    );
    assert!(!objects::process(&w.state, "work:t")["blocked"].is_null());
}

// failed-task-blocks-ac3
#[test]
fn planner_retires_a_blocked_task_and_others_cannot() {
    let mut w = held();
    work(&mut w, "owner", "work:u");
    w.ok("worker", block(&w, "work:t"));
    claim(&mut w, "planner", "criterion:a");
    let retire = |w: &World, key: &str| {
        task_command(
            w,
            "retire_task",
            key,
            json!({"reason":"The planned work replaces this Task"}),
        )
    };
    let cmd = retire(&w, "work:t");
    refuse(&mut w, "other", cmd, OWNER_REQUIRED);
    let cmd = retire(&w, "work:u");
    refuse(&mut w, "planner", cmd, OWNER_REQUIRED);

    // Fixture 4: P retires blocked T and finishes.
    let block_position = w.state.task_process["work:t"]
        .blocked
        .as_ref()
        .unwrap()
        .position;
    let before = w.state.clone();
    let cmd = retire(&w, "work:t");
    w.ok("planner", cmd.clone());
    let mut replayed = before;
    let cmd: Command = serde_json::from_value(cmd).unwrap();
    commands::apply_replay(
        &mut replayed,
        "planner",
        &cmd,
        w.position,
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&replayed).unwrap(),
        serde_json::to_value(&w.state).unwrap()
    );
    assert_eq!(objects::process(&w.state, "work:t")["status"], "retired");
    let history = &w.state.objects["work:t"].revisions;
    assert_eq!(history.len(), 6);
    assert!(
        history
            .iter()
            .any(|r| r.position == block_position && r.reason == "Verification failed")
    );
    let cmd = retire(&w, "work:t");
    refuse(&mut w, "planner", cmd, OWNER_REQUIRED);
    finish_and_replay(&mut w);
    assert_eq!(objects::process(&w.state, "work:t")["status"], "retired");

    // The planner authority ends with the claim.
    let mut w = held();
    w.ok("worker", block(&w, "work:t"));
    let cmd = retire(&w, "work:t");
    refuse(&mut w, "planner", cmd, OWNER_REQUIRED);
}

const PLANNER_OR_OWNER: &str = "planning holder or workspace owner required";

fn unblock(w: &World, key: &str) -> Value {
    task_command(
        w,
        "unblock_task",
        key,
        json!({"reason":"Startup failure; checkout discarded","evidence":"Run: r1"}),
    )
}

/// `actor` unblocks T. Only process.blocked and one revision change, and replay of
/// the same event gives the same state.
fn unblock_and_replay(w: &mut World, actor: &str) {
    let before = w.state.clone();
    let command = unblock(w, "work:t");
    w.ok(actor, command.clone());
    let mut expected = before.task_process["work:t"].clone();
    expected.blocked = None;
    assert_eq!(
        serde_json::to_value(&w.state.task_process["work:t"]).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    let (old, new) = (
        before.objects["work:t"].current(),
        w.state.objects["work:t"].current(),
    );
    assert_eq!(new.revision, old.revision + 1);
    assert_eq!(
        (&new.data, &new.parent, &new.links),
        (&old.data, &old.parent, &old.links)
    );
    assert_eq!(
        (new.actor.as_str(), new.reason.as_str(), new.position),
        (actor, "Startup failure; checkout discarded", w.position)
    );
    let mut replayed = before;
    let cmd: Command = serde_json::from_value(command).unwrap();
    commands::apply_replay(
        &mut replayed,
        actor,
        &cmd,
        w.position,
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&replayed).unwrap(),
        serde_json::to_value(&w.state).unwrap()
    );
}
