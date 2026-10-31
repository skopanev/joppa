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

#[test]
fn migrated_criterion_edits_preserve_native_and_custom_specification_parents() {
    for grouping in ["native", "moved", "custom"] {
        let custom = grouping == "custom";
        let mut w = World::new();
        migrate(&mut w);
        let mut types = objects::schema(&w.state).types;
        types.get_mut("requirement").unwrap().references.insert(
            "audiences".into(),
            objects::ReferenceRule {
                target: "domain".into(),
                title: "Audience".into(),
                multiple: true,
                required: true,
                verification_subject: false,
                allow_all: false,
            },
        );
        if custom {
            types.insert(
                "section".into(),
                serde_json::from_value(
                    json!({"title":"Section","role":"document","status":"none","root":true,"parents":["*"]}),
                )
                .unwrap(),
            );
            types
                .get_mut("requirement")
                .unwrap()
                .parents
                .push("section".into());
        }
        // Add the configured reference to the imported document before validation.
        w.state
            .objects
            .get_mut("requirement:r")
            .unwrap()
            .revisions
            .last_mut()
            .unwrap()
            .data["audiences"] = json!(["d"]);
        w.ok("owner",json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Configure specification context"}));
        w.state
            .objects
            .get_mut("requirement:r")
            .unwrap()
            .revisions
            .last_mut()
            .unwrap()
            .data["custom_context"] = json!({"implementation":"Keep this context"});
        if custom {
            save(
                &mut w,
                "section:outer",
                None,
                json!({"title":"Outer","owner":"owner"}),
            );
            save(
                &mut w,
                "section:inner",
                Some("section:outer"),
                json!({"title":"Inner","owner":"owner"}),
            );
            let data = w.state.objects["requirement:r"].current().data.clone();
            save(&mut w, "requirement:r", Some("section:inner"), data);
        } else if grouping == "moved" {
            save(
                &mut w,
                "capability:other",
                Some("domain:d"),
                json!({"title":"Other grouping","owner":"owner"}),
            );
            let data = w.state.objects["requirement:r"].current().data.clone();
            save(&mut w, "requirement:r", Some("capability:other"), data);
        } else {
            assert!(!w.state.objects["requirement:r"].parent_override);
        }
        let before = w.state.objects["requirement:r"].clone();
        let criterion_ids: Vec<_> = w.state.requirements["r"]
            .revisions
            .last()
            .unwrap()
            .acs
            .iter()
            .map(|ac| ac.id.clone())
            .collect();
        let mut data = w.state.objects["ac:ios"].current().data.clone();
        data["title"] = json!("The iOS client follows the current access contract");
        save(&mut w, "ac:ios", Some("requirement:r"), data);
        let after = w.state.objects["requirement:r"].current();
        assert_eq!(after.parent, before.current().parent);
        assert_eq!(
            w.state.objects["requirement:r"].parent_override,
            before.parent_override
        );
        assert_eq!(after.data, before.current().data);
        assert_eq!(after.links, before.current().links);
        assert_eq!(
            serde_json::to_value(
                &w.state.objects["requirement:r"].revisions[..before.revisions.len()]
            )
            .unwrap(),
            serde_json::to_value(&before.revisions).unwrap()
        );
        assert_eq!(
            w.state.requirements["r"]
                .revisions
                .last()
                .unwrap()
                .acs
                .iter()
                .map(|ac| ac.id.clone())
                .collect::<Vec<_>>(),
            criterion_ids
        );
        assert!(objects::referenced(&w.state, "ac:ios", "domain:d"));
        let current = w.state.objects["ac:ios"].current().clone();
        let state = serde_json::to_value(&w.state).unwrap();
        let position = w.position;
        for (revision, parent) in [
            (current.revision - 1, Some("requirement:r")),
            (current.revision, None),
            (current.revision, Some("domain:d")),
        ] {
            w.no("agent",json!({"op":"save_object","kind":"ac","id":"ios","revision":revision,"parent":parent,"data":current.data,"links":current.links,"reason":"Reject stale or invalid context"}));
            assert_eq!(serde_json::to_value(&w.state).unwrap(), state);
            assert_eq!(w.position, position);
        }
        let parent = w.state.objects["requirement:r"]
            .current()
            .parent
            .clone()
            .unwrap();
        w.no("owner",json!({"op":"archive_object","target":parent,"revision":w.state.objects[&parent].current().revision,"reason":"Reject archive with a present specification"}));
        let parent_kind = w.state.objects[&parent].kind.clone();
        let mut invalid_schema = objects::schema(&w.state).types;
        invalid_schema.remove(&parent_kind);
        w.no("owner",json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":invalid_schema,"reason":"Reject removal of a used parent type"}));
        assert_eq!(serde_json::to_value(&w.state).unwrap(), state);
        assert_eq!(w.position, position);
    }
}
