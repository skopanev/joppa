// Part 1 of tests/spec_rules.rs, included there, so the test names stay unchanged.

fn refused(w: &mut World, actor: &str, command: Value, error: &str) {
    let e = w
        .apply(actor, command.clone())
        .expect_err(&format!("{command} must be refused"));
    assert!(e.to_string().contains(error), "{command}: {e}");
}

/// A note save. Only an object is sent as relations: a save with no edges omits them.
fn note(id: &str, revision: u32, relations: Value) -> Value {
    let mut command = json!({"op":"save_object","kind":"note","id":id,"revision":revision,
        "parent":null,"data":{"title":id,"owner":"owner","body":"Signal"},"reason":"Update"});
    if relations.is_object() {
        command["relations"] = relations;
    }
    command
}

fn schema_types(w: &World) -> Value {
    let mut types = joppa::objects::schema(&w.state).types;
    types.remove("intake");
    types.remove("point");
    serde_json::to_value(types).unwrap()
}

#[test]
fn a_save_on_a_stale_object_revision_is_refused() {
    // edit-makes-revision-ac2
    let mut w = World::new();
    w.ok("agent", note("a", 0, json!({})));
    let mut changed = note("a", 1, json!({}));
    changed["data"]["body"] = json!("Changed signal");
    w.ok("agent", changed);
    refused(
        &mut w,
        "agent",
        note("a", 1, json!({})),
        "stale object revision; read the object again",
    );
    assert_eq!(w.state.objects["note:a"].current().revision, 2);
}

#[test]
fn a_link_with_an_unknown_relation_is_refused() {
    // typed-links-ac2
    let mut w = World::new();
    w.ok("agent", note("a", 0, json!({})));
    w.ok("agent", note("b", 0, json!({})));
    refused(
        &mut w,
        "agent",
        note("b", 1, json!({"likes":{"edges":[{"to":"note:a"}]}})),
        "unknown link relation",
    );
}

#[test]
fn a_blocking_dependency_cycle_is_refused() {
    // blocking-pins-revision-ac2
    let mut w = World::new();
    w.ok("agent", note("a", 0, json!({})));
    w.ok("agent", note("b", 0, json!({})));
    w.ok(
        "agent",
        note(
            "a",
            1,
            json!({"depends_on":{"edges":[{"to":"note:b","revision":1}]}}),
        ),
    );
    refused(
        &mut w,
        "agent",
        note(
            "b",
            1,
            json!({"depends_on":{"edges":[{"to":"note:a","revision":2}]}}),
        ),
        "dependency cycle",
    );
}

#[test]
fn a_link_outside_the_workspace_is_refused() {
    // links-in-workspace-ac1, workspaces-isolated-ac2
    let mut w = World::new();
    w.ok("agent", note("a", 0, json!({})));
    refused(
        &mut w,
        "agent",
        note(
            "a",
            1,
            json!({"related":{"edges":[{"to":"note:only-in-workspace-b"}]}}),
        ),
        "unknown linked object in this workspace",
    );
}

#[test]
fn an_object_with_live_children_cannot_be_archived() {
    // removal-is-archiving-ac5
    let mut w = World::new();
    refused(
        &mut w,
        "owner",
        json!({"op":"archive_object","target":"capability:c","revision":1,"reason":"Remove"}),
        "cannot archive an object with active children",
    );
}

// removal-is-archiving-ac3
#[test]
fn a_human_who_is_not_the_owner_archives() {
    // Capability c holds requirement r, so the test archives an empty capability.
    let mut w = World::new();
    w.legacy(
        "owner",
        json!({"op":"capability","id":"spare","title":"Spare","domain":"d"}),
    );
    let archive =
        json!({"op":"archive_object","target":"capability:spare","revision":1,"reason":"Remove"});
    refused(
        &mut w,
        "stranger",
        archive.clone(),
        "not a workspace member",
    );
    w.ok("legal", archive);
    assert!(!w.state.objects["capability:spare"].current().present);
}

#[test]
fn a_schema_save_on_a_stale_revision_is_refused() {
    // tree-schema-revision-ac2
    let mut w = World::new();
    let types = schema_types(&w);
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Configure"}),
    );
    let e = w
        .apply(
            "owner",
            json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Stale"}),
        )
        .unwrap_err();
    println!("stale schema error: {e}");
    assert!(e.to_string().contains("stale"), "{e}");
    assert_eq!(joppa::objects::schema(&w.state).revision, 1);
}

#[test]
fn a_human_who_is_not_the_owner_changes_the_tree() {
    // tree-owner-sets-types-ac2
    let mut w = World::new();
    let types = schema_types(&w);
    let save = json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Change"});
    refused(&mut w, "stranger", save.clone(), "not a workspace member");
    assert_eq!(joppa::objects::schema(&w.state).revision, 0);
    w.ok("legal", save);
    assert_eq!(joppa::objects::schema(&w.state).revision, 1);
}

#[test]
fn a_requirement_with_no_live_criteria_can_be_archived() {
    // removal-is-archiving-ac2: its criteria go first, then the requirement.
    let mut w = World::new();
    let revision = w.state.objects["requirement:r"].current().revision;
    refused(
        &mut w,
        "owner",
        json!({"op":"archive_object","target":"requirement:r","revision":revision,"reason":"Cut"}),
        "cannot archive an object with active children",
    );
    for ac in ["ac:ios", "ac:key"] {
        let revision = w.state.objects[ac].current().revision;
        w.ok(
            "owner",
            json!({"op":"archive_object","target":ac,"revision":revision,"reason":"Cut"}),
        );
    }
    let revision = w.state.objects["requirement:r"].current().revision;
    w.ok(
        "owner",
        json!({"op":"archive_object","target":"requirement:r","revision":revision,"reason":"Cut"}),
    );
    assert!(!w.state.objects["requirement:r"].current().present);
}

#[test]
fn saving_an_object_adds_exactly_one_revision() {
    // edit-makes-revision-ac1
    let mut w = World::new();
    w.ok("agent", note("a", 0, json!({})));
    let mut next = note("a", 1, json!({}));
    next["data"]["body"] = json!("New text");
    w.ok("agent", next);
    let current = w.state.objects["note:a"].current();
    assert_eq!(
        (current.revision, current.data["body"].as_str()),
        (2, Some("New text"))
    );
}
