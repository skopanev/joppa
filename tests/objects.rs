mod common;
use common::*;
use joppa::objects::{self, ObjectLink, TreeType};
use serde_json::{Value, json};

fn save(w: &mut World, key: &str, parent: Option<&str>, data: Value, links: Vec<ObjectLink>) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = w.state.objects.get(key).map_or(0, |o| o.current().revision);
    w.ok("agent",json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"links":links,"reason":"Update the current document"}));
}
fn link(target: &str, revision: Option<u32>, blocking: bool) -> ObjectLink {
    ObjectLink {
        target: target.into(),
        revision,
        blocking,
        relation: if revision.is_some() {
            "depends_on"
        } else {
            "related"
        }
        .into(),
    }
}
#[test]
fn note_revisions_preserve_source_and_reject_stale_writes() {
    let mut w = World::new();
    save(
        &mut w,
        "note:n",
        None,
        json!({"title":"New source","body":"Original text","owner":"owner"}),
        vec![],
    );
    save(
        &mut w,
        "note:n",
        None,
        json!({"title":"New source","body":"Updated text","owner":"owner"}),
        vec![
            link("domain:d", None, false),
            link("consumer:customer", None, false),
        ],
    );
    let o = &w.state.objects["note:n"];
    assert_eq!(o.revisions.len(), 2);
    assert_eq!(o.revisions[0].data["body"], "Original text");
    assert_eq!(o.current().data["body"], "Updated text");
    assert_eq!(o.current().links.len(), 2);
    w.no("agent",json!({"op":"save_object","kind":"note","id":"n","revision":1,"parent":null,"data":{"title":"Overwrite","owner":"owner"},"reason":"Stale edit"}));
    w.ok(
        "agent",
        json!({"op":"note","target":"note:n","id":"c","text":"Read the source"}),
    );
    assert_eq!(
        w.state.notes["note:n"].comments["c"].at,
        "2026-01-01T00:00:00Z"
    );
}
#[test]
fn schema_supports_custom_grouping_without_a_capability() {
    let mut w = World::new();
    let mut types = objects::defaults();
    types.insert(
        "initiative".into(),
        TreeType {
            references: Default::default(),
            parent_decision: false,
            color: None,
            parent_gate: None,
            title: "Initiative".into(),
            role: "group".into(),
            root: true,
            parents: vec!["initiative".into()],
        },
    );
    types
        .get_mut("requirement")
        .unwrap()
        .parents
        .push("initiative".into());
    w.ok("agent",json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Add an initiative level"}));
    save(
        &mut w,
        "initiative:i",
        None,
        json!({"title":"Release","owner":"owner"}),
        vec![],
    );
    save(
        &mut w,
        "requirement:custom",
        Some("initiative:i"),
        json!({"title":"Custom grouping","body":"Read the release","owner":"owner","consumers":["customer"]}),
        vec![],
    );
    assert_eq!(w.state.objects["requirement:custom"].revisions.len(), 1);
    save(
        &mut w,
        "ac:custom-ac",
        Some("requirement:custom"),
        json!({"title":"The release opens","owner":null,"description":"","depends_on":[],"before_launch":false}),
        vec![],
    );
    let revision = w.state.requirements["custom"]
        .revisions
        .last()
        .unwrap()
        .number;
    w.ok(
        "owner",
        json!({"op":"confirm","req":"custom","revision":revision}),
    );
    w.ok("agent",json!({"op":"assess","req":"custom","revision":revision,"ac":"custom-ac","assessment":{"ready":true,"reason":"Contract is clear","owner":null,"decision":null}}));
    joppa::tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    save(
        &mut w,
        "task:custom/t",
        Some("ac:custom-ac"),
        json!({"title":"Implement the release","contract":"Change the release view","projects":["app"],"purpose":"implementation"}),
        vec![],
    );
    assert!(objects::within(&w.state, "task:custom/t", "initiative:i"));
    assert!(
        joppa::tasks::claimable(
            &w.state,
            "agent",
            "task:custom/t",
            &w.state.task_process["task:custom/t"]
        )
        .is_ok()
    );
    w.no("agent",json!({"op":"set_tree_schema","revision":0,"types":objects::defaults(),"reason":"Stale schema"}));
    let mut invalid = objects::schema(&w.state).types;
    invalid.get_mut("ac").unwrap().parents = vec!["initiative".into()];
    w.no(
        "agent",
        json!({"op":"set_tree_schema","revision":1,"types":invalid,"reason":"Invalid AC parent"}),
    );
}
#[test]
fn ordinary_cycles_are_allowed_and_blocking_cycles_are_rejected() {
    let mut w = World::new();
    let d = w.state.objects["domain:d"].current().data.clone();
    save(
        &mut w,
        "domain:d",
        None,
        d,
        vec![link("domain:d", None, false)],
    );
    let ios = w.state.objects["ac:ios"].current().data.clone();
    save(
        &mut w,
        "ac:ios",
        Some("requirement:r"),
        ios.clone(),
        vec![link("ac:key", Some(1), true)],
    );
    let key = w.state.objects["ac:key"].current().data.clone();
    let n = w.state.objects["ac:key"].current().revision;
    w.no("agent",json!({"op":"save_object","id":"key","kind":"ac","revision":n,"parent":"requirement:r","data":key,"links":[link("ac:ios",Some(2),true)],"reason":"Close a blocking cycle"}));
    save(
        &mut w,
        "ac:ios",
        Some("requirement:r"),
        ios,
        vec![link("ac:key", None, false)],
    );
}
#[test]
fn nonblocking_stale_dependency_flags_review_without_blocking_claim() {
    let mut w = World::new();
    w.ready();
    w.ok("agent",json!({"op":"task","req":"r","revision":1,"id":"t","title":"Implement access","acs":["ios"],"projects":["app"],"purpose":"implementation","contract":"Change the access screen"}));
    save(
        &mut w,
        "note:n",
        None,
        json!({"title":"Context","body":"First","owner":"owner"}),
        vec![],
    );
    let ac = w.state.objects["ac:ios"].current().data.clone();
    save(
        &mut w,
        "ac:ios",
        Some("requirement:r"),
        ac.clone(),
        vec![link("note:n", Some(1), false)],
    );
    save(
        &mut w,
        "note:n",
        None,
        json!({"title":"Context","body":"Second","owner":"owner"}),
        vec![],
    );
    assert_eq!(objects::review_links(&w.state, "ac:ios").len(), 1);
    assert!(objects::work_ready(&w.state, "ac:ios"));
    let r = w.state.requirements["r"].revisions.last().unwrap();
    assert!(joppa::checks::claimable(&w.state, "agent", "r", "t", r, &r.tasks["t"]).is_ok());
    assert_eq!(r.number, 1, "a link does not rewrite the test contract");
}
#[test]
fn blocking_dependencies_require_tests_and_recheck_changed_revisions() {
    let mut w = World::new();
    w.ready();
    let key = w.state.objects["ac:key"].current().data.clone();
    save(
        &mut w,
        "ac:key",
        Some("requirement:r"),
        key.clone(),
        vec![link("ac:ios", Some(1), true)],
    );
    assert!(!objects::work_ready(&w.state, "ac:key"));
    w.run("green", "pass");
    assert!(objects::work_ready(&w.state, "ac:key"));
    let prior = w.state.requirements["r"].revisions[0].runs["green"]
        .results
        .clone();
    let domain = w.state.objects["domain:d"].current().data.clone();
    save(
        &mut w,
        "domain:d",
        None,
        domain,
        vec![link("ac:ios", Some(1), false)],
    );
    assert!(
        objects::work_ready(&w.state, "ac:key"),
        "unrelated metadata keeps evidence"
    );
    w.ok("agent", revision(1));
    assert!(!objects::work_ready(&w.state, "ac:key"));
    assert_eq!(
        w.state.requirements["r"].revisions[0].runs["green"]
            .results
            .len(),
        prior.len()
    );
    assert!(!objects::review_links(&w.state, "ac:key").is_empty());
}
#[test]
fn a_legacy_req_move_updates_the_common_parent_once() {
    let mut w = World::new();
    let data = w.state.objects["requirement:r"].current().data.clone();
    save(&mut w, "requirement:r", Some("capability:c"), data, vec![]);
    assert_eq!(w.state.objects["requirement:r"].revisions.len(), 2);
    w.ok(
        "owner",
        json!({"op":"capability","id":"other","title":"Other capability","domain":"d"}),
    );
    let mut moved = revision(2);
    moved["capability"] = json!("other");
    w.ok("agent", moved);
    assert_eq!(
        w.state.objects["requirement:r"].current().parent.as_deref(),
        Some("capability:other")
    );
    assert_eq!(w.state.objects["requirement:r"].revisions.len(), 3);
}
#[test]
fn ac_annotations_use_one_address_and_completed_tasks_keep_their_ac() {
    let mut w = World::new();
    w.ready();
    w.ok(
        "agent",
        json!({"op":"note","target":"ac:ios","id":"comment","text":"Keep the AC context"}),
    );
    assert!(w.state.notes["ac:r/ios"].comments.contains_key("comment"));
    w.ok("agent",json!({"op":"task","req":"r","revision":1,"id":"t","title":"Implement access","acs":["ios"],"projects":["app"],"purpose":"implementation","contract":"Change the access screen"}));
    w.ok(
        "agent",
        json!({"op":"claim_task","req":"r","revision":1,"id":"t"}),
    );
    w.ok("agent",json!({"op":"complete_task","req":"r","revision":1,"id":"t","evidence":"Merged the change"}));
    let mut next = revision(1);
    next["carry_tasks"] = json!(true);
    w.ok("agent", next);
    w.no("owner",json!({"op":"review_task","req":"r","revision":2,"id":"t","ac":"key","reason":"Move completed evidence"}));
    assert_eq!(
        w.state.requirements["r"].revisions[1].tasks["t"].acs,
        vec!["ios"]
    );
}

#[test]
fn parent_dependencies_cannot_block_their_own_descendants() {
    let mut w = World::new();
    let data = w.state.objects["requirement:r"].current().data.clone();
    let n = w.state.objects["ac:ios"].current().revision;
    w.no("agent",json!({"op":"save_object","kind":"requirement","id":"r","revision":1,"parent":"capability:c","data":data,"links":[link("ac:ios",Some(n),true)],"reason":"Reject inherited blocking cycle"}));
    assert_eq!(w.state.requirements["r"].revisions.len(), 1);
}

#[test]
fn completed_tasks_remain_under_removed_historical_acs() {
    let mut w = World::new();
    w.ready();
    w.ok("agent",json!({"op":"task","req":"r","revision":1,"id":"t","title":"Implement access","acs":["ios"],"projects":["app"],"purpose":"implementation","contract":"Change the access screen"}));
    w.ok(
        "agent",
        json!({"op":"claim_task","req":"r","revision":1,"id":"t"}),
    );
    w.ok("agent",json!({"op":"complete_task","req":"r","revision":1,"id":"t","evidence":"Merged the change"}));
    let mut next = revision(1);
    next["carry_tasks"] = json!(true);
    next["acs"] = json!([{"id":"key","text":"Before production key has exactly read and verify permissions","before_launch":true}]);
    w.ok("agent", next);
    objects::validate_tree(&w.state).unwrap();
    assert!(!w.state.objects["ac:ios"].current().present);
    assert!(w.state.objects["task:r/t"].current().present);
    let task = &w.state.requirements["r"].revisions[1].tasks["t"];
    assert_eq!(task.acs, vec!["ios"]);
    assert_eq!(task.done.as_deref(), Some("Merged the change"));
    assert!(!objects::work_ready(&w.state, "task:r/t"));
    w.no("agent",json!({"op":"note","target":"ac:ios","id":"removed","text":"Cannot annotate an absent current object"}));
}

#[test]
fn capabilities_under_custom_groups_use_normal_owner_rules() {
    let mut w = World::new();
    let mut types = objects::defaults();
    types.insert(
        "initiative".into(),
        TreeType {
            references: Default::default(),
            parent_decision: false,
            color: None,
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
        "agent",
        json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Group capabilities"}),
    );
    save(
        &mut w,
        "initiative:i",
        None,
        json!({"title":"Release","owner":"owner"}),
        vec![],
    );
    save(
        &mut w,
        "capability:custom",
        Some("initiative:i"),
        json!({"title":"Custom capability","owner":"owner","description":"Release scope"}),
        vec![],
    );
    assert_eq!(w.state.capabilities["custom"].domain, None);
    assert_eq!(w.state.objects["capability:custom"].revisions.len(), 1);
    w.no("legal",json!({"op":"save_object","kind":"capability","id":"custom","revision":1,"parent":"initiative:i","data":{"title":"Change owner","owner":"legal"},"reason":"Unauthorized owner transfer"}));
}

#[test]
fn legacy_multiple_ac_tasks_keep_their_original_document_and_binding() {
    let mut w = World::new();
    let task = json!({"op":"task","req":"r","revision":1,"id":"legacy","title":"Old contract","acs":["ios","key"],"projects":["app"],"purpose":"implementation","contract":"Original work"});
    // The old replay path accepted multiple ACs. New writes reject them.
    let command = serde_json::from_value(task.clone()).unwrap();
    joppa::commands::apply_replay(
        &mut w.state,
        "agent",
        &command,
        w.position + 1,
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    let obj = &w.state.objects["task:r/legacy"];
    assert_eq!(obj.current().parent.as_deref(), Some("requirement:r"));
    assert_eq!(obj.current().data["legacy_acs"], json!(["ios", "key"]));
    assert!(obj.current().present);
    objects::validate_tree(&w.state).unwrap();
    let revision = &w.state.requirements["r"].revisions[0];
    assert!(
        joppa::checks::claimable(
            &w.state,
            "agent",
            "r",
            "legacy",
            revision,
            &revision.tasks["legacy"]
        )
        .is_err()
    );
    let mut new = task;
    new["id"] = json!("new");
    w.no("agent", new);
}

#[test]
fn legacy_plans_allow_owner_review_without_a_new_scope_contract() {
    let mut w = World::new();
    w.ready();
    w.run("old", "pass");
    w.ok("agent", revision(1));
    let mut plan = plan("new");
    plan["revision"] = json!(2);
    plan["enforce_coverage"] = json!(true);
    plan["checks"][0]["id"] = json!("[ios] replacement check");
    w.ok("runner", plan);
    let r = &w.state.requirements["r"].revisions[1];
    assert!(!r.coverage_policy);
    assert!(r.verification_scope.is_none());
    assert!(!r.coverage_change.is_empty());
    let review = json!({"op":"review_coverage","req":"r","revision":2,"run":"new","reason":"Replacement check covers the same contract"});
    w.no("agent", review.clone());
    w.ok("owner", review);
    assert_eq!(w.summary(2)["coverage_reviewed"], true);
}

#[test]
fn note_processing_keeps_source_and_reopens_changed_input() {
    let mut w = World::new();
    save(
        &mut w,
        "note:input",
        None,
        json!({"title":"Input","owner":"owner","body":"Original","custom":"Context"}),
        vec![link("domain:d", None, false)],
    );
    w.ok("agent", json!({"op":"set_note_status","target":"note:input","revision":1,"status":"processed","reason":"Updated the linked domain"}));
    let r = w.state.objects["note:input"].current();
    assert_eq!(r.data["body"], "Original");
    assert_eq!(r.data["custom"], "Context");
    assert_eq!(r.links.len(), 1);
    assert_eq!(r.data["status"], "processed");
    w.no("agent", json!({"op":"set_note_status","target":"note:input","revision":1,"status":"unprocessed","reason":"Stale input"}));
    save(
        &mut w,
        "note:input",
        None,
        json!({"title":"Input","owner":"owner","body":"Changed","custom":"Context","status":"processed"}),
        vec![link("domain:d", None, false)],
    );
    assert_eq!(
        w.state.objects["note:input"].current().data["status"],
        "unprocessed"
    );
    assert_eq!(
        w.state.objects["note:input"].revisions[1].data["status"],
        "processed"
    );
    w.no("agent", json!({"op":"set_note_status","target":"domain:d","revision":1,"status":"processed","reason":"Wrong type"}));
    w.no("agent", json!({"op":"set_note_status","target":"note:input","revision":3,"status":"done","reason":"Wrong status"}));
}

#[test]
fn schema_colors_reject_css_values_and_keep_old_schema() {
    let mut w = World::new();
    let mut types = objects::defaults();
    types.get_mut("note").unwrap().color = Some("#7254aB".into());
    w.ok(
        "agent",
        json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Set Note color"}),
    );
    assert_eq!(
        objects::schema(&w.state).types["note"].color.as_deref(),
        Some("#7254aB")
    );
    for color in ["red", "#123", "#123456;display:none", "#12345z"] {
        types.get_mut("note").unwrap().color = Some(color.into());
        w.no("agent", json!({"op":"set_tree_schema","revision":1,"types":types,"reason":"Reject invalid color"}));
    }
    assert_eq!(objects::schema(&w.state).revision, 1);
}
