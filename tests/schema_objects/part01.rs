// Part 1 of tests/schema_objects.rs, included there, so the test names stay unchanged.

fn empty() -> World {
    let mut w = World {
        state: Workspace::default(),
        position: 0,
    };
    w.ok("owner", json!({"op":"workspace","title":"Common objects"}));
    w.ok(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    );
    migrate(&mut w);
    w
}

fn migrate(w: &mut World) {
    for value in [
        json!({"op":"migrate_notes"}),
        json!({"op":"migrate_tasks"}),
        json!({"op":"migrate_schema"}),
        json!({"op":"migrate_documents"}),
    ] {
        let cmd: Command = serde_json::from_value(value).unwrap();
        commands::apply_replay(&mut w.state, "owner", &cmd, w.position + 1, "now").unwrap();
        w.position += 1;
    }
}

fn save(w: &mut World, key: &str, parent: Option<&str>, data: Value) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok("agent",json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Store the common document"}));
}

#[test]
fn required_fields_follow_configured_types_and_value_presence() {
    let mut w = empty();
    let mut types = objects::schema(&w.state).types;
    let names = ["domain", "ac", "task", "note", "feedback"];
    for name in names {
        types.insert(name.into(), serde_json::from_value(json!({"title":name,"role":"document","status":"none","root":true,"parents":["*"],"required_fields":{"origin":"origin is required"}})).unwrap());
    }
    types.insert(
        "record".into(),
        serde_json::from_value(
            json!({"title":"Record","role":"document","status":"none","root":true,"parents":["*"]}),
        )
        .unwrap(),
    );
    w.ok("owner", json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Require an ordinary field on configured types"}));
    assert_eq!(
        serde_json::to_value(&objects::schema(&w.state).types["note"].required_fields).unwrap(),
        json!({"origin":"origin is required"})
    );
    for name in names {
        for value in [
            None,
            Some(Value::Null),
            Some(json!("")),
            Some(json!(" \t\n ")),
            Some(json!([])),
            Some(json!({})),
        ] {
            let mut data = json!({"title":"A document","owner":"owner"});
            if let Some(value) = value {
                data["origin"] = value;
            }
            let before = serde_json::to_value(&w.state).unwrap();
            let error = w.apply("agent", json!({"op":"save_object","kind":name,"id":"empty","revision":0,"parent":null,"data":data,"reason":"Reject an absent value"})).unwrap_err();
            assert_eq!(error.to_string(), "origin is required");
            assert_eq!(serde_json::to_value(&w.state).unwrap(), before);
        }
        for (index, value) in [
            json!(0),
            json!(false),
            json!("origin"),
            json!([0]),
            json!({"value":false}),
        ]
        .into_iter()
        .enumerate()
        {
            let key = format!("{name}:value-{index}");
            save(
                &mut w,
                &key,
                None,
                json!({"title":"A document","owner":"owner","origin":value}),
            );
            assert_eq!(w.state.objects[&key].current().data["origin"], value);
        }
    }
    save(
        &mut w,
        "record:plain",
        None,
        json!({"title":"An ordinary document","owner":"owner"}),
    );
}

#[test]
fn required_field_rules_reject_invalid_configuration_and_default_to_empty() {
    let mut w = empty();
    let current = objects::schema(&w.state);
    for t in current.types.values() {
        assert!(t.required_fields.is_empty());
        assert!(
            serde_json::to_value(t)
                .unwrap()
                .get("required_fields")
                .is_none()
        );
    }
    for rule in [
        json!({"bad field":"Value is required"}),
        json!({"source":" \n "}),
        json!({"verification":"Verification is required"}),
        json!({"text":{"contains":[],"error":"Text is required"}}),
        json!({"text":{"contains":[" \n "],"error":"Text is required"}}),
        json!({"text":{"contains":["marker"],"error":" \n "}}),
        json!({"text":{"contains":["marker"]}}),
        json!({"text":{"contains":["marker"],"error":"Text is required","extra":true}}),
    ] {
        let mut types = serde_json::to_value(&current.types).unwrap();
        types["note"]["required_fields"] = rule;
        let before = serde_json::to_value(&w.state).unwrap();
        w.no("owner", json!({"op":"set_tree_schema","revision":current.revision,"types":types,"reason":"Reject an invalid required field rule"}));
        assert_eq!(serde_json::to_value(&w.state).unwrap(), before);
    }
}

#[test]
fn required_text_rules_use_literal_markers_on_any_document_field() {
    let mut w = empty();
    save(
        &mut w,
        "note:old",
        None,
        json!({"title":"Old input","owner":"owner","body":"Original text"}),
    );
    let original = w.state.objects["note:old"].current().clone();
    w.ok("owner", json!({"op":"archive_object","target":"note:old","revision":1,"reason":"Archive the original input"}));
    let mut types = serde_json::to_value(objects::schema(&w.state).types).unwrap();
    let rule = json!({"contains":["left:","Right:"],"error":"Both markers are required"});
    types["note"]["required_fields"] = json!({"body":rule});
    types["observation"] = json!({"title":"Observation","role":"document","status":"none","root":true,"parents":["*"],"required_fields":{"text":rule}});
    w.ok("owner", json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":types,"reason":"Require configured text markers"}));
    assert_eq!(
        serde_json::to_value(
            objects::schema(&w.state).types["observation"]
                .required_fields
                .clone()
        )
        .unwrap(),
        json!({"text":rule})
    );
    for (kind, field) in [("note", "body"), ("observation", "text")] {
        for invalid in [
            Value::Null,
            json!(false),
            json!(1),
            json!([]),
            json!({"left:":"Right:"}),
            json!(""),
            json!("left:"),
            json!("Right:"),
            json!("Left: Right:"),
            json!("left: right:"),
        ] {
            let mut data = json!({"title":"An observation","owner":"owner"});
            data[field] = invalid;
            let before = serde_json::to_value(&w.state).unwrap();
            let error = w.apply("agent", json!({"op":"save_object","kind":kind,"id":"new","revision":0,"parent":null,"data":data,"reason":"Reject invalid text"})).unwrap_err();
            assert_eq!(error.to_string(), "Both markers are required");
            assert_eq!(serde_json::to_value(&w.state).unwrap(), before);
        }
        let mut data = json!({"title":"An observation","owner":"owner","status":"unprocessed"});
        data[field] = json!("Right: then left:");
        let key = format!("{kind}:new");
        save(&mut w, &key, None, data.clone());
        assert_eq!(w.state.objects[&key].current().data, data);
        data[field] = json!("left: only");
        let error = w.apply("agent", json!({"op":"save_object","kind":kind,"id":"new","revision":1,"parent":null,"data":data,"reason":"Reject removal of a marker"})).unwrap_err();
        assert_eq!(error.to_string(), "Both markers are required");
        assert_eq!(w.state.objects[&key].current().revision, 1);
    }
    assert_eq!(
        serde_json::to_value(&w.state.objects["note:old"].revisions[0]).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    assert!(!w.state.objects["note:old"].current().present);
}
