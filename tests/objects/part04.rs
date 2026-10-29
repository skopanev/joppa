// Part 4 of tests/objects.rs, included there, so the test names stay unchanged.

#[test]
fn archive_configured_documents_preserves_history_and_child_guards() {
    let mut w = archive_world();
    let mut configured = objects::schema(&w.state);
    let mut input = configured.types["note"].clone();
    input.title = "Incoming information".into();
    input.parents = vec!["incoming".into()];
    configured.types.insert("incoming".into(), input);
    w.ok("owner", json!({"op":"set_tree_schema","revision":configured.revision,"types":configured.types,"reason":"Use a custom document type"}));
    save(
        &mut w,
        "incoming:parent",
        None,
        json!({"title":"Review","owner":"owner","body":"Keep history"}),
        vec![link("domain:d", None, false)],
    );
    save(
        &mut w,
        "incoming:child",
        Some("incoming:parent"),
        json!({"title":"Child","owner":"owner"}),
        vec![],
    );
    let original = serde_json::to_value(&w.state.objects["incoming:parent"].revisions[0]).unwrap();
    w.no("owner", json!({"op":"archive_object","target":"incoming:parent","revision":1,"reason":"Reject a live child"}));
    w.ok("owner", json!({"op":"archive_object","target":"incoming:child","revision":1,"reason":"Archive the child"}));
    w.ok("owner", json!({"op":"archive_object","target":"incoming:parent","revision":1,"reason":"Archive the review"}));
    assert!(!w.state.objects["incoming:parent"].current().present);
    assert_eq!(
        serde_json::to_value(&w.state.objects["incoming:parent"].revisions[0]).unwrap(),
        original
    );
    w.no("owner", json!({"op":"restore_object","target":"incoming:child","revision":2,"reason":"Reject an absent parent"}));
    w.no("owner", json!({"op":"restore_object","target":"incoming:parent","revision":1,"reason":"Reject a stale revision"}));
    w.ok("owner", json!({"op":"restore_object","target":"incoming:parent","revision":2,"reason":"Restore the review"}));
    assert_eq!(
        w.state.objects["incoming:parent"].current().data["body"],
        "Keep history"
    );
    assert_eq!(w.state.objects["incoming:parent"].current().links.len(), 1);
    w.ok("owner", json!({"op":"restore_object","target":"incoming:child","revision":2,"reason":"Restore the child"}));
}

#[test]
fn archive_and_restore_group_objects() {
    let mut w = archive_world();
    save(
        &mut w,
        "initiative:i",
        None,
        json!({"title":"Initiative","owner":"owner"}),
        vec![link("domain:d", None, false)],
    );
    w.ok(
        "agent",
        json!({"op":"note","target":"initiative:i","id":"source","text":"Keep this context"}),
    );
    let original = serde_json::to_value(&w.state.objects["initiative:i"].revisions[0]).unwrap();
    w.no("owner", json!({"op":"archive_object","target":"initiative:i","revision":0,"reason":"Archive group"}));
    // Any member archives. The command layer refuses a caller who is not a member.
    w.no("stranger", json!({"op":"archive_object","target":"initiative:i","revision":1,"reason":"Archive group"}));
    w.ok("legal", json!({"op":"archive_object","target":"initiative:i","revision":1,"reason":"Archive group"}));
    assert!(!w.state.objects["initiative:i"].current().present);
    assert_eq!(w.state.objects["initiative:i"].current().revision, 2);
    w.no("owner", json!({"op":"archive_object","target":"initiative:i","revision":2,"reason":"Archive again"}));
    w.no("owner", json!({"op":"restore_object","target":"initiative:i","revision":1,"reason":"Stale restore"}));
    w.ok("owner", json!({"op":"restore_object","target":"initiative:i","revision":2,"reason":"Restore group"}));
    let object = &w.state.objects["initiative:i"];
    assert!(object.current().present);
    assert_eq!(object.current().revision, 3);
    assert_eq!(
        serde_json::to_value(&object.revisions[0]).unwrap(),
        original
    );
    assert_eq!(object.current().links.len(), 1);
    assert_eq!(
        w.state.notes["initiative:i"].comments["source"].text,
        "Keep this context"
    );
}

#[test]
fn archive_requires_an_empty_group_and_restore_requires_a_present_parent() {
    let mut w = archive_world();
    save(
        &mut w,
        "initiative:parent",
        None,
        json!({"title":"Parent","owner":"owner"}),
        vec![],
    );
    save(
        &mut w,
        "initiative:child",
        Some("initiative:parent"),
        json!({"title":"Child","owner":"owner"}),
        vec![],
    );
    w.no("owner", json!({"op":"archive_object","target":"initiative:parent","revision":1,"reason":"Archive parent"}));
    w.no("owner", json!({"op":"archive_object","target":"requirement:r","revision":1,"reason":"Archive a specification"}));
    w.ok("owner", json!({"op":"archive_object","target":"initiative:child","revision":1,"reason":"Archive child"}));
    w.ok("owner", json!({"op":"archive_object","target":"initiative:parent","revision":1,"reason":"Archive empty parent"}));
    w.no("owner", json!({"op":"restore_object","target":"initiative:child","revision":2,"reason":"Restore before parent"}));
    w.ok("owner", json!({"op":"restore_object","target":"initiative:parent","revision":2,"reason":"Restore parent"}));
    w.ok("owner", json!({"op":"restore_object","target":"initiative:child","revision":2,"reason":"Restore child"}));
}

#[test]
fn archive_legacy_group_does_not_resync() {
    let mut w = World::new();
    w.legacy(
        "owner",
        json!({"op":"domain","id":"unused","title":"Unused","owner":"owner"}),
    );
    w.ok("owner", json!({"op":"archive_object","target":"domain:unused","revision":1,"reason":"Archive empty group"}));
    w.legacy(
        "owner",
        json!({"op":"domain","id":"other","title":"Other","owner":"owner"}),
    );
    assert!(!w.state.objects["domain:unused"].current().present);
    assert_eq!(w.state.objects["domain:unused"].current().revision, 2);
    w.ok("owner", json!({"op":"restore_object","target":"domain:unused","revision":2,"reason":"Restore group"}));
    w.legacy(
        "owner",
        json!({"op":"domain","id":"another","title":"Another","owner":"owner"}),
    );
    assert!(w.state.objects["domain:unused"].current().present);
    assert_eq!(w.state.objects["domain:unused"].current().revision, 3);
}

#[test]
fn restore_legacy_group_reflects_edits() {
    let mut w = World::new();
    w.legacy(
        "owner",
        json!({"op":"domain","id":"unused","title":"Unused","owner":"owner"}),
    );
    w.ok("owner", json!({"op":"archive_object","target":"domain:unused","revision":1,"reason":"Archive group"}));
    w.no("owner", json!({"op":"edit_domain","id":"unused","title":"Hidden edit","owner":"owner","reason":"Edit an archived group"}));
    w.ok("owner", json!({"op":"restore_object","target":"domain:unused","revision":2,"reason":"Restore group"}));
    w.legacy("owner", json!({"op":"edit_domain","id":"unused","title":"Changed","owner":"owner","reason":"Edit restored group"}));
    assert!(w.state.objects["domain:unused"].current().present);
    assert_eq!(
        w.state.objects["domain:unused"].current().data["title"],
        "Changed"
    );
    assert_eq!(w.state.objects["domain:unused"].current().revision, 4);
}

#[test]
fn custom_group_fields_survive_archive() {
    let mut w = archive_world();
    save(
        &mut w,
        "initiative:i",
        None,
        json!({"title":"Initiative","owner":"owner","custom":"Preserve this field"}),
        vec![],
    );
    let data = w.state.objects["initiative:i"].current().data.clone();
    let overrides = w.state.document_overrides.clone();
    w.ok("owner", json!({"op":"archive_object","target":"initiative:i","revision":1,"reason":"Archive group"}));
    w.ok("owner", json!({"op":"restore_object","target":"initiative:i","revision":2,"reason":"Restore group"}));
    w.ok(
        "agent",
        json!({"op":"note","target":"domain:d","id":"unrelated","text":"Unrelated context"}),
    );
    assert_eq!(w.state.objects["initiative:i"].current().data, data);
    assert_eq!(w.state.objects["initiative:i"].current().revision, 3);
    assert_eq!(w.state.document_overrides, overrides);
    assert!(w.state.archived_objects.is_empty());
}

// check-list-frozen-ac2
#[test]
fn a_criterion_change_keeps_sibling_readiness_and_an_identical_requirement_save_keeps_all() {
    let mut w = World::new();
    w.ready();
    let state = |w: &World, ac: &str| objects::assessment(&w.state, ac).unwrap()["state"].clone();
    let req = w.state.objects["requirement:r"].current().clone();
    save(
        &mut w,
        "requirement:r",
        req.parent.as_deref(),
        req.data,
        req.links,
    );
    assert_eq!(state(&w, "ac:ios"), "ready");
    assert_eq!(state(&w, "ac:key"), "ready");
    assert!(w.state.criterion_boundaries.is_empty());
    let mut ios = w.state.objects["ac:ios"].current().data.clone();
    ios["title"] = json!("On iOS paid access survives every login");
    save(&mut w, "ac:ios", Some("requirement:r"), ios, vec![]);
    assert_eq!(state(&w, "ac:ios"), "unassessed");
    assert_eq!(
        state(&w, "ac:key"),
        "ready",
        "a sibling keeps its readiness"
    );
    assert_eq!(
        w.state.criterion_boundaries.keys().collect::<Vec<_>>(),
        vec!["ac:ios"]
    );
}

// check-list-frozen-ac2
#[test]
fn a_reset_never_moves_a_boundary_back_in_time() {
    let mut w = World::new();
    let boundary = |w: &World, ac: &str| w.state.criterion_boundaries[ac].clone();
    objects::reset_criteria(&mut w.state, "domain:d", "2026-01-01T12:00:00Z").unwrap();
    // An older reset of an overlapping root keeps the later boundary.
    objects::reset_criteria(&mut w.state, "requirement:r", "2026-01-01T11:00:00Z").unwrap();
    // An equal time in another format keeps the stored one.
    objects::reset_criteria(&mut w.state, "ac:ios", "2026-01-01 12:00:00+00").unwrap();
    for ac in ["ac:ios", "ac:key"] {
        assert_eq!(boundary(&w, ac), "2026-01-01T12:00:00Z");
    }
    objects::reset_criteria(&mut w.state, "ac:ios", "2026-01-01T13:00:00Z").unwrap();
    assert_eq!(boundary(&w, "ac:ios"), "2026-01-01T13:00:00Z");
    assert_eq!(boundary(&w, "ac:key"), "2026-01-01T12:00:00Z");
}
