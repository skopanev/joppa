// Part 2 of tests/readiness_metadata.rs, included there, so the test names stay unchanged.

#[test]
fn dependency_question_blocks_work_and_a_dependent_task() {
    let mut w = World::new();
    let mut next = revision(1);
    next["acs"][0]["depends_on"] = json!(["key"]);
    w.ok("owner", next);
    tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    joppa::body_migration::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    for (ac, ready) in [("ios", true), ("key", false)] {
        let mut cmd = assess(ac, ready);
        cmd["revision"] = json!(2);
        w.ok("agent", cmd);
    }
    save(
        &mut w,
        "task:work",
        Some("ac:ios"),
        work("Implement access"),
    );
    let p = objects::process(&w.state, "task:work");
    assert_eq!(p["assessment"]["state"], "ready");
    assert_eq!(p["readiness_blocker"]["criterion"], "key");
    assert_eq!(p["readiness_blocker"]["recipient"], "legal");
    save(
        &mut w,
        "task:dependent",
        Some("ac:ios"),
        work("Implement the dependent part"),
    );
    let dependent = objects::process(&w.state, "task:dependent");
    assert_eq!(dependent["readiness_blocker"]["criterion"], "key");
    let refusal = tasks::claimable(
        &w.state,
        "legal",
        "task:dependent",
        &w.state.task_process["task:dependent"],
    )
    .unwrap_err()
    .to_string();
    assert!(refusal.contains("Which exact permissions?"));
    assert!(refusal.contains("Recipient: legal"));
}

#[test]
fn custom_role_names_and_parent_changes_select_the_current_assessment() {
    let mut w = World {
        state: Default::default(),
        position: 0,
    };
    w.ok("owner", json!({"op":"workspace","title":"Custom roles"}));
    w.ok("owner", json!({"op":"member","id":"legal","role":"human"}));
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
        "migrate_body",
    ] {
        let cmd = serde_json::from_value(json!({"op":op})).unwrap();
        joppa::commands::apply_replay(&mut w.state, "owner", &cmd, w.position + 1, "now").unwrap();
        w.position += 1;
    }
    assert!(w.state.schema_migrated);
    assert!(w.state.documents_migrated);
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":{
        "spec":{"title":"Specification","role":"requirement","status":"children","root":true},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["*"],"parent_gate":"ready"}
    },"reason":"Configure semantic roles"}),
    );
    for id in ["a", "b"] {
        save(
            &mut w,
            &format!("spec:{id}"),
            None,
            json!({"title":id,"owner":"owner","body":"Current contract"}),
        );
        save(
            &mut w,
            &format!("criterion:{id}"),
            Some(&format!("spec:{id}")),
            json!({"title":"Specified behavior","owner":"owner"}),
        );
    }
    let (req, r, ac) = objects::criterion_context(&w.state, "criterion:a").unwrap();
    let cmd = json!({"op":"assess","req":req,"revision":r.number,"ac":ac.id,"assessment":{
        "ready":false,"reason":"Which account can use this?","recipient":"legal","decision":null}});
    w.ok("owner", cmd);
    save(
        &mut w,
        "work:one",
        Some("criterion:a"),
        work("Implement behavior"),
    );
    let p = objects::process(&w.state, "work:one");
    assert_eq!(p["assessment"]["state"], "waiting");
    assert_eq!(p["assessment"]["req"], "spec:a");
    assert!(
        tasks::claimable(
            &w.state,
            "legal",
            "work:one",
            &w.state.task_process["work:one"]
        )
        .unwrap_err()
        .to_string()
        .contains("Which account")
    );
    save(
        &mut w,
        "work:one",
        Some("criterion:b"),
        work("Implement behavior"),
    );
    let p = objects::process(&w.state, "work:one");
    assert_eq!(p["assessment"]["state"], "unassessed");
    assert_eq!(p["assessment"]["req"], "spec:b");
    assert!(p["assessment"]["reason"].is_null());
    assert_eq!(p["review_required"], true);
}
