// Part 2 of tests/schema_objects.rs, included there, so the test names stay unchanged.

// [product-tree-shape-ac1]
#[test]
fn every_former_type_name_obeys_the_schema_with_identical_document_fields() {
    let mut w = empty();
    let mut types = serde_json::Map::new();
    let names = [
        "domain",
        "capability",
        "consumer",
        "requirement",
        "ac",
        "task",
        "project",
        "note",
        "intake",
        "point",
        "custom",
    ];
    for name in names {
        types.insert(
            name.into(),
            json!({"title":name,"role":"document","status":"none","root":true,"parents":["*"]}),
        );
    }
    w.ok("owner",json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Names do not select behavior"}));
    let data = json!({"title":"The same document","owner":"owner","body":"Common text","body":"Common description","consumers":{"ordinary":"data"},"projects":"ordinary data","status":"ordinary"});
    for name in names {
        let key = format!("{name}:same");
        save(&mut w, &key, None, data.clone());
        assert_eq!(w.state.objects[&key].current().data, data);
        assert!(!objects::input_type(&w.state, name));
        w.legacy(
            "agent",
            json!({"op":"describe","target":key,"text":"Same annotation","reason":"Add context"}),
        );
        assert_eq!(w.state.notes[&key].description, "Same annotation");
        assert_eq!(w.state.objects[&key].current().revision, 1);
    }
    assert!(w.state.domains.is_empty());
    assert!(w.state.capabilities.is_empty());
    assert!(w.state.consumers.is_empty());
    assert!(w.state.requirements.is_empty());
    assert!(w.state.task_process.is_empty());
    let mut types = objects::schema(&w.state).types;
    types.get_mut("custom").unwrap().inbox = Some(true);
    w.ok("owner",json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Enable only the configured input queue"}));
    save(
        &mut w,
        "custom:input",
        None,
        json!({"title":"Incoming fact","owner":"owner"}),
    );
    w.ok("owner",json!({"op":"decide_note","target":"custom:input","revision":1,"outcome":"rejected","reason":"Reviewed the fact"}));
    assert_eq!(
        objects::process(&w.state, "custom:input")["status"],
        "processed"
    );
    assert!(!objects::input_type(&w.state, "note"));
}

// [ac:restructure-keeps-evidence-ac4]
#[test]
fn document_migration_preserves_all_revisions_and_runs_once() {
    let mut w = World::new();
    objects::migrate_notes(&mut w.state, "owner", w.position + 1, "now").unwrap();
    tasks::migrate(&mut w.state, "owner", w.position + 2, "now").unwrap();
    joppa::body_migration::migrate(&mut w.state, "owner", w.position + 2, "now").unwrap();
    objects::migrate_schema(&mut w.state, "owner", w.position + 3, "now").unwrap();
    let old = serde_json::to_value(&w.state.objects).unwrap();
    objects::migrate_documents(&mut w.state, "owner", w.position + 4, "now").unwrap();
    assert_eq!(serde_json::to_value(&w.state.objects).unwrap(), old);
    let current = serde_json::to_value(&w.state).unwrap();
    objects::migrate_documents(&mut w.state, "owner", w.position + 5, "later").unwrap();
    assert_eq!(serde_json::to_value(&w.state).unwrap(), current);
    let mut data = w.state.objects["domain:d"].current().data.clone();
    data["extra"] = json!({"preserved":true});
    save(&mut w, "domain:d", None, data.clone());
    assert_eq!(w.state.objects["domain:d"].current().data, data);
    assert_eq!(
        w.state.objects["domain:d"].revisions[0].data,
        old["domain:d"]["revisions"][0]["data"]
    );
}

