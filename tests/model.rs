mod common;
use common::*;
use serde_json::json;

#[test]
fn only_workspace_owner_can_rename_without_changing_content() {
    let mut w = World::new();
    let command =
        json!({"op":"rename_workspace","title":"New product name","reason":"Product naming"});
    w.no("agent", command.clone());
    w.no("legal", command.clone());
    w.no(
        "owner",
        json!({"op":"rename_workspace","title":" ","reason":"Product naming"}),
    );
    let mut expected = serde_json::to_value(&w.state).unwrap();
    expected["title"] = json!("New product name");
    w.ok("owner", command);
    assert_eq!(serde_json::to_value(&w.state).unwrap(), expected);
}

#[test]
fn creator_can_correct_unclaimed_task_without_revising_req() {
    let mut w = World::new();
    w.legacy("agent",json!({"op":"task","req":"r","revision":1,"id":"wrong","title":"Build flow","acs":["ios"],"projects":["infra"],"purpose":"implementation","contract":"Build the iOS flow"}));
    let amend = json!({"op":"amend_task","req":"r","revision":1,"id":"wrong","title":"Build mobile flow","contract":"Build the iOS flow in app and attach build evidence","projects":["app"],"reason":"Correct project before claim"});
    w.no("runner", amend.clone());
    w.ok("agent", amend);
    assert_eq!(
        w.state.requirements["r"].revisions[0].tasks["wrong"].projects,
        vec!["app"]
    );
    w.ok("owner",json!({"op":"retire_task","req":"r","revision":1,"id":"wrong","reason":"Superseded by another implementation"}));
    w.no(
        "agent",
        json!({"op":"claim_task","req":"r","revision":1,"id":"wrong"}),
    );
}
#[test]
fn revision_keeps_tasks_and_requires_owner_review_before_more_work() {
    let mut w = World::new();
    w.ok("agent", assess("ios", true));
    for id in ["finished", "ongoing"] {
        w.legacy("agent", json!({"op":"task","req":"r","revision":1,"id":id,"title":"Implement access","acs":["ios"],"projects":["app"],"purpose":"implementation","assignee":"agent"}));
        if id == "finished" {
            w.ok("agent",json!({"op":"complete_task","commit":"c0ffee0","req":"r","revision":1,"id":"finished","evidence":"merged build abc"}));
        }
    }
    let mut next = revision(1);
    next["carry_tasks"] = json!(true);
    w.ok("agent", next);
    let r = &w.state.requirements["r"].revisions[1];
    assert_eq!(r.tasks.len(), 2);
    assert_eq!(
        r.tasks["finished"].done.as_deref(),
        Some("merged build abc")
    );
    assert!(r.tasks.values().all(|task| task.review_required));
    assert_eq!(r.tasks["ongoing"].origin_revision, 1);
    let complete = json!({"op":"complete_task","commit":"c0ffee0","req":"r","revision":2,"id":"ongoing","evidence":"merged build def"});
    w.no("agent", complete.clone());
    let review = json!({"op":"review_task","req":"r","revision":2,"id":"ongoing","ac":"ios","reason":"Still implements unchanged iOS condition"});
    w.no("agent", review.clone());
    w.ok("owner", review);
    w.no("agent", complete.clone()); // New AC assessment is still required.
    let mut a = assess("ios", true);
    a["revision"] = json!(2);
    w.ok("agent", a);
    w.ok("agent", complete);
    assert_eq!(
        w.state.requirements["r"].revisions[1].tasks["ongoing"]
            .done
            .as_deref(),
        Some("merged build def")
    );
    assert_eq!(
        w.state.requirements["r"].revisions[0].tasks["ongoing"].done,
        None
    );
}
#[test]
fn later_revision_recovers_historical_tasks_and_owner_can_retire_one() {
    let mut w = World::new();
    w.ok("agent", assess("ios", true));
    w.legacy("agent", json!({"op":"task","req":"r","revision":1,"id":"old","title":"Old implementation","acs":["ios"],"projects":["app"],"purpose":"implementation","assignee":"agent"}));
    w.legacy("agent", revision(1)); // Historical revise event: no task-carry marker.
    assert!(w.state.requirements["r"].revisions[1].tasks.is_empty());
    let mut third = revision(2);
    third["carry_tasks"] = json!(true);
    w.ok("agent", third);
    assert_eq!(
        w.state.requirements["r"].revisions[2].tasks["old"].origin_revision,
        1
    );
    let retire = json!({"op":"retire_task","req":"r","revision":3,"id":"old","reason":"AC changed; old implementation is obsolete"});
    w.no("agent", retire.clone());
    w.ok("owner", retire);
    assert_eq!(
        w.state.requirements["r"].revisions[2].tasks["old"]
            .retired
            .as_deref(),
        Some("AC changed; old implementation is obsolete")
    );
    w.no(
        "agent",
        json!({"op":"complete_task","commit":"c0ffee0","req":"r","revision":3,"id":"old","evidence":"merged"}),
    );
    let mut fourth = revision(3);
    fourth["carry_tasks"] = json!(true);
    w.ok("agent", fourth);
    assert!(!w.state.requirements["r"].revisions[3].tasks["old"].review_required);
    assert!(
        w.state.requirements["r"].revisions[3].tasks["old"]
            .retired
            .is_some()
    );
}
#[test]
fn specialist_decision_cannot_be_impersonated() {
    let mut w = World::new();
    let mut rev = revision(1);
    rev["acs"][1]["decider"] = json!("legal");
    w.ok("agent", rev);
    let mut assess = assess("key", true);
    assess["revision"] = json!(2);
    w.no("agent", assess.clone());
    let mut c = json!({"op":"comment","req":"r","revision":2,"id":"decision","text":"read and verify only","kind":"decision","reply_to":null,"ac":"key"});
    w.no("agent", c.clone());
    w.ok("owner", c.clone());
    assess["assessment"]["decision"] = json!("decision");
    w.no("agent", assess.clone());
    c["id"] = json!("legal-decision");
    w.ok("legal", c);
    assess["assessment"]["decision"] = json!("legal-decision");
    w.ok("agent", assess);
}
#[test]
fn tree_cycles_blank_acs_unauthorized_users_and_spurious_scope_are_rejected() {
    let mut w = World::new();
    w.no("stranger", assess("ios", true));
    w.no(
        "agent",
        json!({"op":"capability","id":"bad","title":"Wrong parent","domain":"c"}),
    );
    let mut rev = revision(1);
    rev["acs"][0]["text"] = json!("   ");
    w.no("agent", rev);
    let mut rev = revision(1);
    rev["acs"][0]["depends_on"] = json!(["key"]);
    rev["acs"][1]["depends_on"] = json!(["ios"]);
    w.no("agent", rev);
    let mut rev = revision(1);
    rev["acs"][0]["platforms"] = json!(["android"]);
    w.no("agent", rev);
}
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
