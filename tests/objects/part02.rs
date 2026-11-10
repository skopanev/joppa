// Part 2 of tests/objects.rs, included there, so the test names stay unchanged.

/// Save `key` with `relations` at its current revision, or create it.
fn save_relations(w: &mut World, key: &str, parent: Option<&str>, data: Value, relations: Value) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok("agent",json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"relations":relations,"reason":"Update the current document"}));
}

#[test]
fn parent_dependencies_cannot_block_their_own_descendants() {
    let mut w = World::new();
    let data = w.state.objects["requirement:r"].current().data.clone();
    let n = w.state.objects["ac:ios"].current().revision;
    w.no("agent",json!({"op":"save_object","kind":"requirement","id":"r","revision":1,"parent":"capability:c","data":data,"relations":{"depends_on":{"edges":[{"to":"ac:ios","revision":n}]}},"reason":"Reject inherited blocking cycle"}));
    assert_eq!(w.state.requirements["r"].revisions.len(), 1);
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
            fields: Default::default(),
            parent_decision: false,
            status: None,
            process_fields: None,
            inbox: None,
            source: false,
            launch_blocker: None,
            color: None,
            title_size: None,
            badge: None,
            title_preview_length: None,
            field_max_length: None,
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
    save_relations(
        &mut w,
        "initiative:i",
        None,
        json!({"title":"Release","owner":"owner"}),
        json!({}),
    );
    save_relations(
        &mut w,
        "capability:custom",
        Some("initiative:i"),
        json!({"title":"Custom capability","owner":"owner","description":"Release scope"}),
        json!({}),
    );
    assert_eq!(w.state.capabilities["custom"].domain, None);
    assert_eq!(w.state.objects["capability:custom"].revisions.len(), 1);
    // legal is a human member who did not create the workspace: the save succeeds.
    w.ok("legal",json!({"op":"save_object","kind":"capability","id":"custom","revision":1,"parent":"initiative:i","data":{"title":"Change owner","owner":"legal"},"reason":"Owner transfer by a human member"}));
    let current = w.state.objects["capability:custom"].current();
    assert_eq!(current.data["owner"], "legal");
    assert_eq!(w.state.objects["capability:custom"].revisions.len(), 2);
    assert_eq!(w.state.capabilities["custom"].domain, None);
}

// [ask-only-missing-ac3]
#[test]
fn note_processing_keeps_source_and_reopens_changed_input() {
    let mut w = World::new();
    save_relations(
        &mut w,
        "note:input",
        None,
        json!({"title":"Input","owner":"owner","body":"Original","custom":"Context"}),
        json!({"related":{"edges":[{"to":"domain:d"}]}}),
    );
    // An old journal event decides the input: live writes refuse decide_note, a replay reads it.
    w.ok("agent", json!({"op":"decide_note","target":"note:input","revision":1,"outcome":"changed_spec","reason":"Updated the linked domain","links":["domain:d"]}));
    let r = w.state.objects["note:input"].current();
    assert_eq!(r.data["body"], "Original");
    assert_eq!(r.data["custom"], "Context");
    let stored = serde_json::to_value(r).unwrap();
    assert_eq!(
        stored["relations"]["related"]["edges"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(r.data["status"], "processed");
    save_relations(
        &mut w,
        "note:input",
        None,
        json!({"title":"Input","owner":"owner","body":"Changed","custom":"Context","status":"processed"}),
        json!({}),
    );
    assert_eq!(
        w.state.objects["note:input"].current().data["status"],
        "unprocessed"
    );
    assert_eq!(
        w.state.objects["note:input"].revisions[1].data["status"],
        "processed"
    );
}
