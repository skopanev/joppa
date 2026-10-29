// Part 2 of tests/objects.rs, included there, so the test names stay unchanged.

#[test]
fn a_legacy_req_move_updates_the_common_parent_once() {
    let mut w = World::new();
    let mut data = w.state.objects["requirement:r"].current().data.clone();
    data["title"] = json!("Paid access survives login");
    save(&mut w, "requirement:r", Some("capability:c"), data, vec![]);
    assert_eq!(w.state.objects["requirement:r"].revisions.len(), 2);
    w.legacy(
        "owner",
        json!({"op":"capability","id":"other","title":"Other capability","domain":"d"}),
    );
    let mut moved = revision(2);
    moved["capability"] = json!("other");
    w.ok("agent", moved);
    assert_eq!(
        w.state.objects["requirement:r"].current().parent.as_deref(),
        Some("capability:other")
    );
    assert_eq!(w.state.objects["requirement:r"].revisions.len(), 3);
}

#[test]
fn ac_annotations_use_one_address_and_completed_tasks_keep_their_ac() {
    let mut w = World::new();
    w.ready();
    w.ok(
        "agent",
        json!({"op":"note","target":"ac:ios","id":"comment","text":"Keep the AC context"}),
    );
    assert!(w.state.notes["ac:r/ios"].comments.contains_key("comment"));
    w.legacy("agent",json!({"op":"task","req":"r","revision":1,"id":"t","title":"Implement access","acs":["ios"],"projects":["app"],"purpose":"implementation","contract":"Change the access screen"}));
    w.ok(
        "agent",
        json!({"op":"claim_task","req":"r","revision":1,"id":"t"}),
    );
    w.ok("agent",json!({"op":"complete_task","commit":"c0ffee0","req":"r","revision":1,"id":"t","evidence":"Merged the change"}));
    let mut next = revision(1);
    next["carry_tasks"] = json!(true);
    w.ok("agent", next);
    w.no("owner",json!({"op":"review_task","req":"r","revision":2,"id":"t","ac":"key","reason":"Move completed evidence"}));
    assert_eq!(
        w.state.requirements["r"].revisions[1].tasks["t"].acs,
        vec!["ios"]
    );
}

#[test]
fn parent_dependencies_cannot_block_their_own_descendants() {
    let mut w = World::new();
    let data = w.state.objects["requirement:r"].current().data.clone();
    let n = w.state.objects["ac:ios"].current().revision;
    w.no("agent",json!({"op":"save_object","kind":"requirement","id":"r","revision":1,"parent":"capability:c","data":data,"links":[link("ac:ios",Some(n),true)],"reason":"Reject inherited blocking cycle"}));
    assert_eq!(w.state.requirements["r"].revisions.len(), 1);
}

#[test]
fn completed_tasks_remain_under_removed_historical_acs() {
    let mut w = World::new();
    w.ready();
    w.legacy("agent",json!({"op":"task","req":"r","revision":1,"id":"t","title":"Implement access","acs":["ios"],"projects":["app"],"purpose":"implementation","contract":"Change the access screen"}));
    w.ok(
        "agent",
        json!({"op":"claim_task","req":"r","revision":1,"id":"t"}),
    );
    w.ok("agent",json!({"op":"complete_task","commit":"c0ffee0","req":"r","revision":1,"id":"t","evidence":"Merged the change"}));
    let mut next = revision(1);
    next["carry_tasks"] = json!(true);
    next["acs"] = json!([{"id":"key","text":"Before production key has exactly read and verify permissions","before_launch":true}]);
    w.ok("agent", next);
    objects::validate_tree(&w.state).unwrap();
    assert!(!w.state.objects["ac:ios"].current().present);
    assert!(w.state.objects["task:r/t"].current().present);
    let task = &w.state.requirements["r"].revisions[1].tasks["t"];
    assert_eq!(task.acs, vec!["ios"]);
    assert_eq!(task.done.as_deref(), Some("Merged the change"));
    assert!(!objects::work_ready(&w.state, "task:r/t"));
    w.no("agent",json!({"op":"note","target":"ac:ios","id":"removed","text":"Cannot annotate an absent current object"}));
}

#[test]
fn capabilities_under_custom_groups_use_normal_owner_rules() {
    let mut w = World::new();
    let mut types = objects::defaults();
    types.insert(
        "initiative".into(),
        TreeType {
            question: None,
            index: None,
            references: Default::default(),
            required_fields: Default::default(),
            parent_decision: false,
            status: None,
            status_order: None,
            process_fields: None,
            inbox: None,
            launch_blocker: None,
            color: None,
            title_size: None,
            badge: None,
            title_preview_length: None,
            parent_gate: None,
            title: "Initiative".into(),
            role: "group".into(),
            root: true,
            parents: vec![],
        },
    );
    types
        .get_mut("capability")
        .unwrap()
        .parents
        .push("initiative".into());
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Group capabilities"}),
    );
    save(
        &mut w,
        "initiative:i",
        None,
        json!({"title":"Release","owner":"owner"}),
        vec![],
    );
    save(
        &mut w,
        "capability:custom",
        Some("initiative:i"),
        json!({"title":"Custom capability","owner":"owner","description":"Release scope"}),
        vec![],
    );
    assert_eq!(w.state.capabilities["custom"].domain, None);
    assert_eq!(w.state.objects["capability:custom"].revisions.len(), 1);
    w.no("legal",json!({"op":"save_object","kind":"capability","id":"custom","revision":1,"parent":"initiative:i","data":{"title":"Change owner","owner":"legal"},"reason":"Unauthorized owner transfer"}));
}

#[test]
fn legacy_multiple_ac_tasks_keep_their_original_document_and_binding() {
    let mut w = World::new();
    let task = json!({"op":"task","req":"r","revision":1,"id":"legacy","title":"Old contract","acs":["ios","key"],"projects":["app"],"purpose":"implementation","contract":"Original work"});
    // The old replay path accepted multiple ACs. New writes reject them.
    let command = serde_json::from_value(task.clone()).unwrap();
    joppa::commands::apply_replay(
        &mut w.state,
        "agent",
        &command,
        w.position + 1,
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    let obj = &w.state.objects["task:r/legacy"];
    assert_eq!(obj.current().parent.as_deref(), Some("requirement:r"));
    assert_eq!(obj.current().data["legacy_acs"], json!(["ios", "key"]));
    assert!(obj.current().present);
    objects::validate_tree(&w.state).unwrap();
    let revision = &w.state.requirements["r"].revisions[0];
    assert!(
        joppa::checks::claimable(
            &w.state,
            "agent",
            "r",
            "legacy",
            revision,
            &revision.tasks["legacy"]
        )
        .is_err()
    );
    let mut new = task;
    new["id"] = json!("new");
    w.no("agent", new);
}

// [ask-only-missing-ac3]
#[test]
fn note_processing_keeps_source_and_reopens_changed_input() {
    let mut w = World::new();
    save(
        &mut w,
        "note:input",
        None,
        json!({"title":"Input","owner":"owner","body":"Original","custom":"Context"}),
        vec![link("domain:d", None, false)],
    );
    w.ok("agent", json!({"op":"decide_note","target":"note:input","revision":1,"outcome":"changed_spec","reason":"Updated the linked domain","links":["domain:d"]}));
    let r = w.state.objects["note:input"].current();
    assert_eq!(r.data["body"], "Original");
    assert_eq!(r.data["custom"], "Context");
    assert_eq!(r.links.len(), 1);
    assert_eq!(r.data["status"], "processed");
    w.no("agent", json!({"op":"decide_note","target":"note:input","revision":1,"outcome":"changed_spec","reason":"Stale input","links":["domain:d"]}));
    save(
        &mut w,
        "note:input",
        None,
        json!({"title":"Input","owner":"owner","body":"Changed","custom":"Context","status":"processed"}),
        vec![link("domain:d", None, false)],
    );
    assert_eq!(
        w.state.objects["note:input"].current().data["status"],
        "unprocessed"
    );
    assert_eq!(
        w.state.objects["note:input"].revisions[1].data["status"],
        "processed"
    );
    w.no("agent", json!({"op":"decide_note","target":"domain:d","revision":1,"outcome":"changed_spec","reason":"Wrong type","links":["domain:d"]}));
    w.no("agent", json!({"op":"decide_note","target":"note:input","revision":3,"outcome":"done","reason":"Wrong outcome"}));
}
