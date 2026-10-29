// Part 2 of tests/spec_rules.rs, included there, so the test names stay unchanged.

#[test]
fn a_link_keeps_its_relation_type() {
    // typed-links-ac1
    let mut w = World::new();
    w.ok("agent", note("a", 0, json!([])));
    w.ok(
        "agent",
        note(
            "b",
            0,
            json!([{"target":"note:a","relation":"depends_on","revision":1}]),
        ),
    );
    let link = &w.state.objects["note:b"].current().links[0];
    assert_eq!(
        (link.target.as_str(), link.relation.as_str()),
        ("note:a", "depends_on")
    );
}

#[test]
fn a_move_keeps_the_address_and_a_forbidden_parent_is_refused() {
    // id-survives-move-ac2 (and the move half of ac1)
    let mut w = World::new();
    w.legacy(
        "owner",
        json!({"op":"capability","id":"c2","title":"Second","domain":"d"}),
    );
    let move_to = |w: &World, parent: &str| {
        let r = w.state.objects["requirement:r"].current();
        json!({"op":"save_object","kind":"requirement","id":"r","revision":r.revision,"parent":parent,"data":r.data,"links":[],"reason":"Move"})
    };
    let command = move_to(&w, "capability:c2");
    w.ok("agent", command);
    assert_eq!(
        w.state.objects["requirement:r"].current().parent.as_deref(),
        Some("capability:c2")
    );
    let command = move_to(&w, "domain:d");
    refused(
        &mut w,
        "agent",
        command,
        "parent type is not permitted by the tree schema",
    );
    assert_eq!(
        w.state.objects["requirement:r"].current().parent.as_deref(),
        Some("capability:c2")
    );
}

#[test]
fn the_owner_adds_a_type_and_the_schema_gets_one_revision() {
    // tree-owner-sets-types-ac1, tree-schema-revision-ac1
    let mut w = World::new();
    let mut types = schema_types(&w);
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Base"}),
    );
    types["risk"] = json!({"title":"Risk","role":"document","status":"none","root":false,"parents":["capability"],"references":{"consumers":{"target":"consumer","title":"Used by","multiple":true,"required":false,"verification_subject":false}}});
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":1,"types":types,"reason":"Add risk"}),
    );
    let schema = joppa::objects::schema(&w.state);
    assert_eq!(schema.revision, 2);
    assert_eq!(schema.types["risk"].parents, vec!["capability".to_string()]);
    assert_eq!(
        schema.types["risk"].references["consumers"].target,
        "consumer"
    );
    assert!(
        w.state
            .tree_schema
            .iter()
            .any(|s| s.revision == 1 && !s.types.contains_key("risk")),
        "revision 1 still reads whole"
    );
}

#[test]
fn removing_a_type_that_has_objects_is_refused() {
    // tree-schema-safe-ac1
    let mut w = World::new();
    let types = schema_types(&w);
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Base"}),
    );
    w.ok("agent", note("a", 0, json!([])));
    let mut without = types.clone();
    without.as_object_mut().unwrap().remove("note");
    refused(
        &mut w,
        "owner",
        json!({"op":"set_tree_schema","revision":1,"types":without,"reason":"Remove"}),
        "object type is absent from the tree schema",
    );
    assert!(w.state.objects["note:a"].current().present);
}

#[test]
fn a_parent_rule_that_orphans_objects_is_refused() {
    // tree-schema-safe-ac2
    let mut w = World::new();
    let types = schema_types(&w);
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Base"}),
    );
    let mut orphaning = types.clone();
    orphaning["requirement"]["parents"] = json!(["domain"]);
    refused(
        &mut w,
        "owner",
        json!({"op":"set_tree_schema","revision":1,"types":orphaning,"reason":"Narrow"}),
        "parent type is not permitted by the tree schema",
    );
}
