// Part 2 of tests/model.rs, included there, so the test names stay unchanged.

#[test]
fn source_history_migrates_to_notes_without_losing_context() {
    use joppa::{
        commands::{self, Command},
        objects,
    };
    let mut w = World::new();
    let said = "  сломалось\n\nplease help  ";
    let replay = |w: &mut World, value: serde_json::Value| {
        let cmd: Command = serde_json::from_value(value).unwrap();
        commands::apply_replay(
            &mut w.state,
            "agent",
            &cmd,
            w.position + 1,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
        w.position += 1;
    };
    replay(
        &mut w,
        json!({"op":"intake","id":"i","said":said,"author":"customer"}),
    );
    replay(&mut w, json!({"op":"split","id":"i","points":[said]}));
    replay(
        &mut w,
        json!({"op":"sort","id":"i","point":"i.1","kind":"bug"}),
    );
    for field in ["observed", "expected", "where"] {
        replay(
            &mut w,
            json!({"op":"answer","id":"i","point":"i.1","field":field,"value":"literal answer"}),
        );
    }
    replay(
        &mut w,
        json!({"op":"route","id":"i","point":"i.1","outcome":"work","reason":"Matches existing behavior","req":"r"}),
    );
    w.ok(
        "agent",
        json!({"op":"note","target":"point:i/i.1","id":"context","text":"Keep this context"}),
    );
    let old_history = serde_json::to_value(&w.state.objects["point:i/i.1"].revisions).unwrap();
    let req = serde_json::to_value(&w.state.requirements["r"]).unwrap();
    commands::apply_replay(
        &mut w.state,
        "owner",
        &Command::MigrateNotes {},
        w.position + 1,
        "2026-01-02T00:00:00Z",
    )
    .unwrap();
    w.position += 1;
    assert!(w.state.intake.is_empty() && w.state.questions.is_empty());
    assert!(!objects::schema(&w.state).types.contains_key("intake"));
    assert!(!objects::schema(&w.state).types.contains_key("point"));
    let source = objects::resolve_address(&w.state, "intake:i").to_owned();
    let point = objects::resolve_address(&w.state, "point:i/i.1").to_owned();
    assert_eq!(w.state.objects[&source].kind, "note");
    assert_eq!(w.state.objects[&source].current().data["body"], said);
    assert_eq!(w.state.objects[&point].current().data["body"], said);
    assert_eq!(w.state.objects[&point].current().data["author"], "customer");
    assert_eq!(
        w.state.objects[&point].current().data["review_policy"],
        json!(["observed", "expected", "where"])
    );
    assert_eq!(
        w.state.objects[&point].current().parent.as_deref(),
        Some(source.as_str())
    );
    assert_eq!(
        w.state.notes[&point].comments["context"].text,
        "Keep this context"
    );
    let link = &w.state.objects[&point].current().links[0];
    assert_eq!(link.target, "requirement:r");
    assert_eq!(link.relation, "related");
    assert!(link.revision.is_none() && !link.blocking);
    assert_eq!(
        serde_json::to_value(&w.state.requirements["r"]).unwrap(),
        req
    );
    let len = old_history.as_array().unwrap().len();
    assert_eq!(
        serde_json::to_value(&w.state.objects["point:i/i.1"].revisions[..len]).unwrap(),
        old_history
    );
    assert_eq!(
        serde_json::to_value(&w.state.objects[&point].revisions[..len]).unwrap(),
        old_history
    );
    let obj = w.state.objects[&point].clone();
    let mut data = obj.current().data.clone();
    data["body"] = json!("Corrected source text");
    let save = json!({"op":"save_object","kind":"note","id":obj.id,"revision":obj.current().revision,"parent":"intake:i","data":data,"links":obj.current().links,"reason":"Correct the source"});
    let mut cycle = save.clone();
    cycle["parent"] = json!("point:i/i.1");
    w.no("owner", cycle);
    w.ok("owner", save);
    assert!(objects::work_ready(&w.state, &point));
    assert!(objects::within(&w.state, &point, "intake:i"));
    assert_eq!(
        w.state.objects[&point].current().revision,
        obj.current().revision + 1
    );
    w.ok(
        "agent",
        json!({"op":"note","target":"point:i/i.1","id":"followup","text":"Follow up"}),
    );
    assert_eq!(w.state.notes[&point].comments.len(), 2);
    for value in [
        json!({"op":"intake","id":"new","said":"New text","author":"customer"}),
        json!({"op":"split","id":"i","points":[said]}),
        json!({"op":"set_tree_schema","revision":objects::schema(&w.state).revision,"types":objects::legacy_defaults(),"reason":"Restore source types"}),
    ] {
        w.no("owner", value);
    }
    let state = serde_json::to_value(&w.state).unwrap();
    objects::migrate_notes(&mut w.state, "owner", w.position + 1, "later").unwrap();
    assert_eq!(serde_json::to_value(&w.state).unwrap(), state);
}

#[test]
fn archive_clears_unused_tree_without_losing_ids_or_allowing_hidden_work() {
    let mut w = World::new();
    w.legacy(
        "owner",
        json!({"op":"domain","id":"seed","title":"Example","authority":"owner"}),
    );
    w.legacy(
        "owner",
        json!({"op":"capability","id":"seed-cap","title":"Example ability","domain":"seed"}),
    );
    let archive = json!({"op":"set_domain_archived","id":"seed","archived":true,"reason":"Start a fresh tree"});
    w.no("agent", archive.clone());
    w.no("legal", archive.clone());
    w.ok("owner", archive.clone());
    assert!(w.state.domains["seed"].archived);
    assert!(w.state.capabilities.contains_key("seed-cap"));
    assert_eq!(
        joppa::tree_order::ordered_ids(&w.state, "domain", None).unwrap(),
        vec!["d"]
    );
    w.no(
        "owner",
        json!({"op":"domain","id":"seed","title":"Replacement","authority":"owner"}),
    );
    w.no(
        "owner",
        json!({"op":"capability","id":"hidden","title":"Hidden work","domain":"seed"}),
    );
    let mut req = revision(0);
    req["req"] = json!("seed-req");
    req["capability"] = json!("seed-cap");
    req["acs"] = json!([{"id":"seed-ac","text":"An observable result"}]);
    w.no("agent", req.clone());
    let mut restore = archive.clone();
    restore["archived"] = json!(false);
    w.ok("owner", restore);
    w.ok("agent", req);
    w.no("owner", archive);
    assert_eq!(w.state.requirements["seed-req"].revisions.len(), 1);
}
