// Part 1 of tests/planning_claims.rs, included there, so the test names stay unchanged.

fn configured() -> World {
    let mut w = World {
        state: Workspace::default(),
        position: 0,
    };
    w.ok(
        "owner",
        json!({"op":"workspace","title":"Planning contracts"}),
    );
    for actor in ["planner", "other", "worker"] {
        w.ok("owner", json!({"op":"member","id":actor,"role":"human"}));
    }
    // Only the workspace owner sets the tree schema.
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":0,"reason":"Configure roles","types":{
            "spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":[]},
            "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
            "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
        }}),
    );
    objects::migrate_schema(&mut w.state, "owner", w.position + 1, "now").unwrap();
    tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    joppa::body_migration::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    objects::migrate_documents(&mut w.state, "owner", w.position + 1, "now").unwrap();
    save(
        &mut w,
        "planner",
        "spec:one",
        None,
        json!({"title":"One rule","body":"Implement the agreed rule","owner":"owner"}),
    );
    for ac in ["criterion:a", "criterion:b"] {
        save(
            &mut w,
            "planner",
            ac,
            Some("spec:one"),
            json!({"title":ac,"owner":"owner"}),
        );
    }
    let revision = w.state.requirements["spec:one"]
        .revisions
        .last()
        .unwrap()
        .number;
    w.ok(
        "owner",
        json!({"op":"confirm","req":"spec:one","revision":revision}),
    );
    for ac in ["criterion:a", "criterion:b"] {
        w.ok("planner", json!({"op":"assess","req":"spec:one","revision":revision,"ac":ac,"assessment":{"ready":true,"reason":"Known scope"}}));
    }
    save(
        &mut w,
        "planner",
        "spec:two",
        None,
        json!({"title":"Two rule","body":"Implement the second rule","owner":"owner"}),
    );
    save(
        &mut w,
        "planner",
        "criterion:c",
        Some("spec:two"),
        json!({"title":"criterion:c","owner":"owner"}),
    );
    let revision = w.state.requirements["spec:two"]
        .revisions
        .last()
        .unwrap()
        .number;
    w.ok(
        "owner",
        json!({"op":"confirm","req":"spec:two","revision":revision}),
    );
    w.ok("planner", json!({"op":"assess","req":"spec:two","revision":revision,"ac":"criterion:c","assessment":{"ready":true,"reason":"Known scope"}}));
    w
}

fn save(w: &mut World, actor: &str, key: &str, parent: Option<&str>, data: Value) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok(actor, json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Set the contract"}));
}

fn work(w: &mut World, actor: &str, id: &str) {
    save(
        w,
        actor,
        id,
        Some("criterion:a"),
        json!({"title":id,"owner":"owner","body":"Implement this criterion","purpose":"implementation"}),
    );
}

fn claim(w: &mut World, actor: &str, ac: &str) {
    w.ok(
        actor,
        json!({"op":"claim_planning","ac":ac,"revision":w.state.objects[ac].current().revision}),
    );
}

fn refuse(w: &mut World, actor: &str, cmd: Value, expected: &str) {
    let before = serde_json::to_value(&w.state).unwrap();
    let position = w.position;
    assert_eq!(w.apply(actor, cmd).unwrap_err().to_string(), expected);
    assert_eq!(w.position, position);
    assert_eq!(serde_json::to_value(&w.state).unwrap(), before);
}

// planning-claim-ac1
#[test]
fn claim_guards_are_atomic_and_existing_work_does_not_block() {
    let mut w = configured();
    work(&mut w, "planner", "work:old");
    let revision = w.state.objects["criterion:a"].current().revision;
    refuse(
        &mut w,
        "planner",
        json!({"op":"claim_planning","ac":"criterion:a","revision":revision-1}),
        "stale object revision; read the object again",
    );
    refuse(
        &mut w,
        "planner",
        json!({"op":"claim_planning","ac":"criterion:missing","revision":1}),
        "unknown criterion",
    );
    refuse(
        &mut w,
        "planner",
        json!({"op":"claim_planning","ac":"work:old","revision":1}),
        "planning requires a criterion",
    );
    claim(&mut w, "planner", "criterion:a");
    assert_eq!(
        objects::process(&w.state, "criterion:a")["planning_claim"],
        json!({"actor":"planner","position":w.position})
    );
    refuse(
        &mut w,
        "other",
        json!({"op":"claim_planning","ac":"criterion:a","revision":revision}),
        "criterion is already claimed for planning",
    );
    // A planner may hold claims in several REQs.
    claim(&mut w, "planner", "criterion:c");
    assert_eq!(
        objects::process(&w.state, "criterion:c")["planning_claim"]["actor"],
        "planner"
    );
    assert_eq!(w.state.planning_claims["criterion:a"].actor, "planner");
    let mut other_workspace = configured();
    claim(&mut other_workspace, "planner", "criterion:c");
}

// planning-claim-ac8
#[test]
fn one_planner_claims_and_finishes_every_criterion_of_one_req() {
    let mut w = configured();
    claim(&mut w, "planner", "criterion:a");
    claim(&mut w, "planner", "criterion:b");
    for ac in ["criterion:a", "criterion:b"] {
        assert_eq!(
            objects::process(&w.state, ac)["planning_claim"]["actor"],
            "planner"
        );
    }
    for (id, ac) in [("work:a", "criterion:a"), ("work:b", "criterion:b")] {
        save(
            &mut w,
            "planner",
            id,
            Some(ac),
            json!({"title":id,"owner":"owner","body":"Implement this criterion","purpose":"implementation"}),
        );
    }
    for ac in ["criterion:a", "criterion:b"] {
        w.ok(
            "planner",
            json!({"op":"finish_planning","ac":ac,"reason":"Plan complete"}),
        );
        assert!(objects::process(&w.state, ac)["planning_claim"].is_null());
    }
    for id in ["work:a", "work:b"] {
        assert!(tasks::claimable(&w.state, "worker", id, &w.state.task_process[id]).is_ok());
    }
}
