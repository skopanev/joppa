// Part 4 of tests/planning_claims.rs, included there, so the test names stay unchanged.

// failed-task-blocks-ac1
#[test]
fn owner_blocks_and_finished_tasks_cannot_be_blocked() {
    let mut w = held();
    work(&mut w, "planner", "work:u");
    w.ok("other", task_command(&w, "claim_task", "work:u", json!({})));
    w.ok("owner", block(&w, "work:u"));
    let p = objects::process(&w.state, "work:u");
    assert_eq!(
        (p["status"].as_str(), p["blocked"]["actor"].as_str()),
        (Some("open"), Some("owner"))
    );
    assert!(p["assignee"].is_null());

    work(&mut w, "planner", "work:done");
    w.ok(
        "other",
        task_command(&w, "claim_task", "work:done", json!({})),
    );
    w.ok(
        "other",
        task_command(
            &w,
            "complete_task",
            "work:done",
            json!({"evidence":"The work is complete"}),
        ),
    );
    let cmd = block(&w, "work:done");
    refuse(&mut w, "owner", cmd, "completed Task cannot be blocked");
    work(&mut w, "planner", "work:retired");
    w.ok(
        "owner",
        task_command(
            &w,
            "retire_task",
            "work:retired",
            json!({"reason":"The work is no longer required"}),
        ),
    );
    let cmd = block(&w, "work:retired");
    refuse(&mut w, "owner", cmd, "retired Task cannot be blocked");
}

// failed-task-blocks-ac1
#[test]
fn claims_refuse_a_blocked_task_and_documents_cannot_change_the_block() {
    let mut w = held();
    w.ok("worker", block(&w, "work:t"));
    let blocked = objects::process(&w.state, "work:t")["blocked"].clone();
    for actor in ["worker", "other"] {
        let cmd = task_command(&w, "claim_task", "work:t", json!({}));
        refuse(&mut w, actor, cmd, BLOCKED);
        let t = &w.state.task_process["work:t"];
        assert_eq!(
            tasks::claimable(&w.state, actor, "work:t", t)
                .unwrap_err()
                .to_string(),
            BLOCKED
        );
    }
    // Membership comes before the block guard.
    let t = &w.state.task_process["work:t"];
    assert_eq!(
        tasks::claimable(&w.state, "stranger", "work:t", t)
            .unwrap_err()
            .to_string(),
        "claimant is not a workspace member"
    );

    // A document write can neither set nor clear the block.
    let document = json!({"title":"work:t","owner":"owner","contract":"Implement this criterion","purpose":"implementation"});
    for value in [Value::Null, json!({"actor":"owner"})] {
        let mut data = document.clone();
        data["blocked"] = value;
        let before = serde_json::to_value(&w.state).unwrap();
        let position = w.position;
        let revision = w.state.objects["work:t"].current().revision;
        assert!(w.apply("owner", json!({"op":"save_object","kind":"work","id":"t","revision":revision,"parent":"criterion:a","data":data,"reason":"Change the block"})).is_err());
        assert_eq!(
            (w.position, serde_json::to_value(&w.state).unwrap()),
            (position, before)
        );
    }
    let mut data = document;
    data["contract"] = json!("Implement this criterion again");
    save(&mut w, "owner", "work:t", Some("criterion:a"), data);
    assert_eq!(objects::process(&w.state, "work:t")["blocked"], blocked);

    // Publication comes before the block guard.
    claim(&mut w, "planner", "criterion:a");
    work(&mut w, "planner", "work:new");
    w.ok("owner", block(&w, "work:new"));
    let t = &w.state.task_process["work:new"];
    assert_eq!(
        tasks::claimable(&w.state, "other", "work:new", t)
            .unwrap_err()
            .to_string(),
        "Task is awaiting planning publication"
    );
}

const NEEDS_CONTRACT: &str = "blocked Task needs a new contract or retirement";

const OWNER_REQUIRED: &str = "active Task owner required";

fn finish() -> Value {
    json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"})
}

/// L blocks T under ready A. P (`planner`) then claims A.
fn blocked_plan() -> World {
    let mut w = held();
    w.ok("worker", block(&w, "work:t"));
    claim(&mut w, "planner", "criterion:a");
    w
}

/// `actor` saves T with `data` and `links`.
fn save_t(w: &mut World, actor: &str, data: Value, links: Value) {
    let revision = w.state.objects["work:t"].current().revision;
    w.ok(actor, json!({"op":"save_object","kind":"work","id":"t","revision":revision,"parent":"criterion:a","data":data,"links":links,"reason":"Change the contract"}));
}

fn t_document() -> Value {
    json!({"title":"work:t","owner":"owner","contract":"Implement this criterion","purpose":"implementation"})
}

fn dependency(target: &str) -> Value {
    json!({"target":target,"relation":"depends_on","revision":1,"blocking":true})
}

/// P finishes A. Replay of the finish event gives the same state.
fn finish_and_replay(w: &mut World) {
    let before = w.state.clone();
    w.ok("planner", finish());
    let mut replayed = before;
    let cmd: Command = serde_json::from_value(finish()).unwrap();
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
    assert!(objects::process(&w.state, "criterion:a")["planning_claim"].is_null());
}

// failed-task-blocks-ac3
#[test]
fn finish_refuses_an_unchanged_blocked_task_and_keeps_the_claim_and_block() {
    let mut w = blocked_plan();
    let blocked = objects::process(&w.state, "work:t")["blocked"].clone();
    // Fixture 3: this refusal comes before "planning produced no Task changes".
    refuse(&mut w, "planner", finish(), NEEDS_CONTRACT);
    // An identical save and a change to other fields are not a new contract.
    save_t(&mut w, "planner", t_document(), json!([]));
    refuse(&mut w, "planner", finish(), NEEDS_CONTRACT);
    let mut described = t_document();
    described["description"] = json!("Explain the failed verification");
    save_t(&mut w, "planner", described, json!([]));
    refuse(&mut w, "planner", finish(), NEEDS_CONTRACT);
    // A new unrelated Task does not change T.
    work(&mut w, "planner", "work:new");
    refuse(&mut w, "planner", finish(), NEEDS_CONTRACT);
    assert_eq!(w.state.planning_claims["criterion:a"].actor, "planner");
    assert_eq!(objects::process(&w.state, "work:t")["blocked"], blocked);
    // Release, amendment and document saves keep the block.
    w.ok(
        "planner",
        json!({"op":"release_planning","ac":"criterion:a","reason":"Plan withdrawn"}),
    );
    assert_eq!(objects::process(&w.state, "work:t")["blocked"], blocked);
    let cmd = task_command(
        &w,
        "amend_task",
        "work:t",
        json!({"title":"work:t","contract":"Implement the criterion again","projects":[],"reason":"Amend the contract"}),
    );
    w.ok("owner", cmd);
    assert_eq!(objects::process(&w.state, "work:t")["blocked"], blocked);
}

// failed-task-blocks-ac3
#[test]
fn finish_clears_the_block_after_a_new_title_or_contract() {
    for field in ["title", "contract"] {
        let mut w = blocked_plan();
        // Fixture 1: P amends the T contract.
        let mut amend = json!({"title":"work:t","contract":"Implement this criterion","projects":[],"reason":"Amend the contract"});
        amend[field] = json!("Implement the criterion with a new fix");
        let cmd = task_command(&w, "amend_task", "work:t", amend);
        w.ok("planner", cmd);
        finish_and_replay(&mut w);
        assert!(objects::process(&w.state, "work:t")["blocked"].is_null());
        let t = &w.state.task_process["work:t"];
        assert!(tasks::claimable(&w.state, "other", "work:t", t).is_ok());
    }
}

// failed-task-blocks-ac3
#[test]
fn finish_clears_the_block_after_a_new_dependency_and_names_it_in_the_waiting_reason() {
    let mut w = blocked_plan();
    // Fixture 2: P adds Task C under A and a blocking link from T to C.
    work(&mut w, "planner", "work:c");
    save_t(
        &mut w,
        "planner",
        t_document(),
        json!([dependency("work:c")]),
    );
    finish_and_replay(&mut w);
    assert!(objects::process(&w.state, "work:t")["blocked"].is_null());
    let t = &w.state.task_process["work:t"];
    let reason = tasks::claimable(&w.state, "other", "work:t", t)
        .unwrap_err()
        .to_string();
    assert!(reason.contains("work:c") && reason != BLOCKED, "{reason}");
}
