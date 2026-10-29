// Part 3 of tests/objects.rs, included there, so the test names stay unchanged.

#[test]
fn schema_title_sizes_preserve_documents_and_old_schemas() {
    let mut w = World::new();
    save(
        &mut w,
        "note:font",
        None,
        json!({"title":"Saved input","owner":"owner"}),
        vec![],
    );
    let documents = serde_json::to_value(&w.state.objects).unwrap();
    let mut old = objects::schema(&w.state);
    assert!(old.types.values().all(|t| t.title_size.is_none()));
    old.types.remove("intake");
    old.types.remove("point");
    let mut types = old.types.clone();
    types.get_mut("note").unwrap().title_size = Some(21);
    w.ok("owner", json!({"op":"set_tree_schema","revision":old.revision,"types":types,"reason":"Set type title size"}));
    assert_eq!(objects::schema(&w.state).types["note"].title_size, Some(21));
    assert_eq!(documents, serde_json::to_value(&w.state.objects).unwrap());
    for size in [0, 11, 49, -1] {
        let mut invalid = serde_json::to_value(&types).unwrap();
        invalid["note"]["title_size"] = json!(size);
        w.no("owner", json!({"op":"set_tree_schema","revision":1,"types":invalid,"reason":"Reject invalid title size"}));
    }
    assert_eq!(objects::schema(&w.state).revision, 1);
    w.ok("owner", json!({"op":"set_tree_schema","revision":1,"types":old.types,"reason":"Restore the original presentation"}));
    assert!(objects::schema(&w.state).types["note"].title_size.is_none());
    assert_eq!(w.state.tree_schema[0].types["note"].title_size, Some(21));
}

#[test]
fn schema_badges_and_previews_preserve_full_document_titles() {
    let mut w = World::new();
    let title = "A complete saved title with business context that remains unchanged when the preview becomes short";
    // The title is written while the type still allows it; the shorter limit later must not rewrite it.
    let mut types = objects::schema(&w.state).types;
    types.remove("intake");
    types.remove("point");
    types.get_mut("note").unwrap().title_preview_length = Some(256);
    w.ok("owner",json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Allow long note titles"}));
    save(
        &mut w,
        "note:preview",
        None,
        json!({"title":title,"owner":"owner","body":"Original context"}),
        vec![],
    );
    let before = serde_json::to_value(&w.state.objects).unwrap();
    types.get_mut("note").unwrap().badge = Some("INPUT".into());
    types.get_mut("note").unwrap().title_preview_length = Some(64);
    w.ok("owner",json!({"op":"set_tree_schema","revision":1,"types":types,"reason":"Configure compact titles and badges"}));
    assert_eq!(before, serde_json::to_value(&w.state.objects).unwrap());
    assert_eq!(
        w.state.objects["note:preview"].current().data["title"],
        title
    );
    for badge in ["", "TOO-LONG-LABEL", "REQ CAP", "REQ\n"] {
        let mut invalid = types.clone();
        invalid.get_mut("note").unwrap().badge = Some(badge.into());
        w.no("owner",json!({"op":"set_tree_schema","revision":2,"types":invalid,"reason":"Reject invalid badge"}));
    }
    for length in [0, 257, -1] {
        let mut invalid = serde_json::to_value(&types).unwrap();
        invalid["note"]["title_preview_length"] = json!(length);
        w.no("owner",json!({"op":"set_tree_schema","revision":2,"types":invalid,"reason":"Reject invalid title preview"}));
    }
    assert_eq!(objects::schema(&w.state).revision, 2);
    assert_eq!(
        objects::schema(&w.state).types["note"].badge.as_deref(),
        Some("INPUT")
    );
}

#[test]
fn schema_colors_reject_css_values_and_keep_old_schema() {
    let mut w = World::new();
    let mut types = objects::defaults();
    types.get_mut("note").unwrap().color = Some("#7254aB".into());
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Set Note color"}),
    );
    assert_eq!(
        objects::schema(&w.state).types["note"].color.as_deref(),
        Some("#7254aB")
    );
    for color in ["red", "#123", "#123456;display:none", "#12345z"] {
        types.get_mut("note").unwrap().color = Some(color.into());
        w.no("owner", json!({"op":"set_tree_schema","revision":1,"types":types,"reason":"Reject invalid color"}));
    }
    assert_eq!(objects::schema(&w.state).revision, 1);
}

// [restructure-keeps-evidence-ac1]
#[test]
fn type_order_changes_schema_without_changing_documents() {
    let mut w = World::new();
    save(
        &mut w,
        "note:order",
        None,
        json!({"title":"Saved input","owner":"owner","body":"Preserve this text"}),
        vec![],
    );
    let before = serde_json::to_value(&w.state.objects).unwrap();
    let old = objects::schema(&w.state);
    assert!(old.types.values().all(|t| t.index.is_none()));
    let mut types = old.types.clone();
    types.remove("intake");
    types.remove("point");
    types.get_mut("domain").unwrap().index = Some(0);
    types.get_mut("consumer").unwrap().index = Some(10);
    w.ok("owner", json!({"op":"set_tree_schema","revision":old.revision,"types":types,"reason":"Order root collections"}));
    let current = objects::schema(&w.state);
    assert_eq!(current.revision, old.revision + 1);
    assert_eq!(current.types["domain"].index, Some(0));
    assert_eq!(before, serde_json::to_value(&w.state.objects).unwrap());
    w.no("owner", json!({"op":"set_tree_schema","revision":old.revision,"types":types,"reason":"Reject a stale schema edit"}));
    let mut invalid = serde_json::to_value(&types).unwrap();
    invalid["domain"]["index"] = json!(-1);
    w.no("owner", json!({"op":"set_tree_schema","revision":current.revision,"types":invalid,"reason":"Reject an invalid index"}));
}

// [ac:unrouted-requests-listed-ac2]
#[test]
fn configured_inputs_process_without_a_note_name_and_reopen_on_edits() {
    let mut w = World::new();
    let mut types = objects::defaults();
    types.insert(
        "incoming".into(),
        serde_json::from_value(
            json!({"title":"Incoming","role":"document","status":"none","root":true,"parents":["*"],"inbox":true}),
        )
        .unwrap(),
    );
    types.get_mut("note").unwrap().inbox = Some(false);
    w.ok("owner",json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Select the input queue"}));
    save(
        &mut w,
        "incoming:n",
        None,
        json!({"title":"Provider change","owner":"owner","source":"New permission rules"}),
        vec![],
    );
    w.ok("owner",json!({"op":"decide_note","target":"incoming:n","revision":1,"outcome":"rejected","reason":"Reviewed source"}));
    assert_eq!(
        objects::process(&w.state, "incoming:n")["status"],
        "processed"
    );
    save(
        &mut w,
        "incoming:n",
        None,
        json!({"title":"Provider change","owner":"owner","source":"Changed permission rules","status":"processed"}),
        vec![],
    );
    assert_eq!(
        objects::process(&w.state, "incoming:n")["status"],
        "unprocessed"
    );
    assert_eq!(
        w.state.objects["incoming:n"].revisions[0].data["source"],
        "New permission rules"
    );
    save(
        &mut w,
        "note:ordinary",
        None,
        json!({"title":"Ordinary document","owner":"owner"}),
        vec![],
    );
    w.no("owner",json!({"op":"decide_note","target":"note:ordinary","revision":1,"outcome":"rejected","reason":"This type is not an inbox"}));
}

#[test]
fn launch_configuration_counts_blockers_and_can_disable_the_preset() {
    let mut w = World::new();
    let baseline = objects::launch_summary(&w.state);
    assert_eq!(baseline["blocker_count"], 1);
    w.ready();
    // Only an accept_commit acceptance on a green record makes the launch ready.
    assert_eq!(objects::launch_summary(&w.state)["ready"], false);
    let mut types = objects::defaults();
    types.get_mut("ac").unwrap().launch_blocker = Some(false);
    w.ok("owner",json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Remove launch conditions from this type"}));
    assert_eq!(objects::launch_summary(&w.state)["configured"], false);
}

fn archive_world() -> World {
    let mut w = World::new();
    let mut types = objects::defaults();
    let mut group = types["domain"].clone();
    group.title = "Initiative".into();
    group.parents = vec!["initiative".into()];
    types.insert("initiative".into(), group);
    w.ok("owner", json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Configure group nesting"}));
    w
}
