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
// [request-verbatim-ac1]
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
            index: None,
            references: Default::default(),
            parent_decision: false,
            inbox: None,
            launch_blocker: None,
            color: None,
            title_size: None,
            badge: None,
            title_preview_length: None,
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
// [ac:all-object-links-ac3]
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
// [ac:dependency-evidence-ac1]
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
            index: None,
            references: Default::default(),
            parent_decision: false,
            inbox: None,
            launch_blocker: None,
            color: None,
            title_size: None,
            badge: None,
            title_preview_length: None,
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

// [ask-only-missing-ac3]
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
    w.ok("agent", json!({"op":"set_tree_schema","revision":old.revision,"types":types,"reason":"Set type title size"}));
    assert_eq!(objects::schema(&w.state).types["note"].title_size, Some(21));
    assert_eq!(documents, serde_json::to_value(&w.state.objects).unwrap());
    for size in [0, 11, 49, -1] {
        let mut invalid = serde_json::to_value(&types).unwrap();
        invalid["note"]["title_size"] = json!(size);
        w.no("agent", json!({"op":"set_tree_schema","revision":1,"types":invalid,"reason":"Reject invalid title size"}));
    }
    assert_eq!(objects::schema(&w.state).revision, 1);
    w.ok("agent", json!({"op":"set_tree_schema","revision":1,"types":old.types,"reason":"Restore the original presentation"}));
    assert!(objects::schema(&w.state).types["note"].title_size.is_none());
    assert_eq!(w.state.tree_schema[0].types["note"].title_size, Some(21));
}

#[test]
fn schema_badges_and_previews_preserve_full_document_titles() {
    let mut w = World::new();
    let title = "A complete saved title with business context that remains unchanged when the preview becomes short";
    save(
        &mut w,
        "note:preview",
        None,
        json!({"title":title,"owner":"owner","body":"Original context"}),
        vec![],
    );
    let before = serde_json::to_value(&w.state.objects).unwrap();
    let mut types = objects::schema(&w.state).types;
    types.remove("intake");
    types.remove("point");
    types.get_mut("note").unwrap().badge = Some("INPUT".into());
    types.get_mut("note").unwrap().title_preview_length = Some(64);
    w.ok("agent",json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Configure compact titles and badges"}));
    assert_eq!(before, serde_json::to_value(&w.state.objects).unwrap());
    assert_eq!(
        w.state.objects["note:preview"].current().data["title"],
        title
    );
    for badge in ["", "TOO-LONG-LABEL", "REQ CAP", "REQ\n"] {
        let mut invalid = types.clone();
        invalid.get_mut("note").unwrap().badge = Some(badge.into());
        w.no("agent",json!({"op":"set_tree_schema","revision":1,"types":invalid,"reason":"Reject invalid badge"}));
    }
    for length in [0, 257, -1] {
        let mut invalid = serde_json::to_value(&types).unwrap();
        invalid["note"]["title_preview_length"] = json!(length);
        w.no("agent",json!({"op":"set_tree_schema","revision":1,"types":invalid,"reason":"Reject invalid title preview"}));
    }
    assert_eq!(objects::schema(&w.state).revision, 1);
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
    w.ok("agent", json!({"op":"set_tree_schema","revision":old.revision,"types":types,"reason":"Order root collections"}));
    let current = objects::schema(&w.state);
    assert_eq!(current.revision, old.revision + 1);
    assert_eq!(current.types["domain"].index, Some(0));
    assert_eq!(before, serde_json::to_value(&w.state.objects).unwrap());
    w.no("agent", json!({"op":"set_tree_schema","revision":old.revision,"types":types,"reason":"Reject a stale schema edit"}));
    let mut invalid = serde_json::to_value(&types).unwrap();
    invalid["domain"]["index"] = json!(-1);
    w.no("agent", json!({"op":"set_tree_schema","revision":current.revision,"types":invalid,"reason":"Reject an invalid index"}));
}

// [ac:unrouted-requests-listed-ac2]
#[test]
fn configured_inputs_process_without_a_note_name_and_reopen_on_edits() {
    let mut w = World::new();
    let mut types = objects::defaults();
    types.insert(
        "incoming".into(),
        serde_json::from_value(
            json!({"title":"Incoming","role":"document","root":true,"parents":["*"],"inbox":true}),
        )
        .unwrap(),
    );
    types.get_mut("note").unwrap().inbox = Some(false);
    w.ok("agent",json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Select the input queue"}));
    save(
        &mut w,
        "incoming:n",
        None,
        json!({"title":"Provider change","owner":"owner","source":"New permission rules"}),
        vec![],
    );
    w.ok("agent",json!({"op":"set_note_status","target":"incoming:n","revision":1,"status":"processed","reason":"Reviewed source"}));
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
    w.no("agent",json!({"op":"set_note_status","target":"note:ordinary","revision":1,"status":"processed","reason":"This type is not an inbox"}));
}

#[test]
fn launch_configuration_preserves_owner_acceptance_and_can_disable_the_preset() {
    let mut w = World::new();
    let baseline = objects::launch_summary(&w.state);
    assert_eq!(baseline["blocker_count"], 1);
    w.ready();
    w.run("pass", "pass");
    assert_eq!(objects::launch_summary(&w.state)["ready"], false);
    w.ok(
        "owner",
        json!({"op":"accept","req":"r","revision":1,"run":"pass"}),
    );
    assert_eq!(objects::launch_summary(&w.state)["ready"], true);
    let mut types = objects::defaults();
    types.get_mut("ac").unwrap().launch_blocker = Some(false);
    w.ok("agent",json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Remove launch conditions from this type"}));
    assert_eq!(objects::launch_summary(&w.state)["configured"], false);
}

// [ac:dependency-evidence-ac2]
#[test]
fn requirement_dependencies_follow_tests_and_completed_work_loses_verification() {
    let mut w = World::new();
    w.ready();
    joppa::tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    let pin = w.state.objects["requirement:r"].current().revision;
    save(
        &mut w,
        "task:dependent",
        Some("ac:ios"),
        json!({"title":"Implement access","owner":"owner","contract":"Implement access and record the result","purpose":"implementation","projects":[]}),
        vec![link("requirement:r", Some(pin), true)],
    );
    w.no(
        "agent",
        json!({"op":"claim_task","id":"task:dependent","revision":1}),
    );
    let run = |w: &mut World, id: &str, outcome: &str| {
        let mut checks = plan(id);
        checks["checks"][0]["tasks"] = json!([{"address":"task:dependent","origin_revision":1}]);
        w.ok("runner", checks);
        w.ok(
            "runner",
            result(id, "[ios] paid access", outcome, vec!["access retained"]),
        );
        w.ok(
            "owner",
            result(id, "key-permissions", "pass", vec!["read", "verify"]),
        );
        w.ok(
            "runner",
            json!({"op":"finish","req":"r","revision":1,"run":id}),
        );
    };
    run(&mut w, "first", "pass");
    assert_eq!(w.summary(1)["accepted"], false);
    assert!(objects::work_ready(&w.state, "task:dependent"));
    w.ok(
        "agent",
        json!({"op":"claim_task","id":"task:dependent","revision":1}),
    );
    w.ok("agent",json!({"op":"complete_task","id":"task:dependent","revision":2,"evidence":"Access implementation delivered"}));
    let completion = w.state.task_process["task:dependent"].done.clone();
    assert_eq!(
        objects::process(&w.state, "task:dependent")["verification"],
        "pass"
    );
    run(&mut w, "second", "fail");
    assert!(!objects::work_ready(&w.state, "task:dependent"));
    let process = objects::process(&w.state, "task:dependent");
    assert_eq!(process["status"], "done");
    assert_eq!(process["verification"], "unverified");
    assert_eq!(w.state.task_process["task:dependent"].done, completion);
}

#[test]
fn common_document_fields_survive_process_commands_and_full_replacement_removes_them() {
    let mut w = World::new();
    objects::migrate_schema(&mut w.state, "owner", w.position + 1, "now").unwrap();
    let baseline = w.state.objects["requirement:r"].revisions.clone();
    let mut data = w.state.objects["requirement:r"].current().data.clone();
    data["rationale"] = json!({"source":"customer interview","priority":3});
    data["labels"] = json!(["mobile", "access"]);
    save(
        &mut w,
        "requirement:r",
        Some("capability:c"),
        data.clone(),
        vec![],
    );
    let rev = w.state.requirements["r"].revisions.last().unwrap().number;
    w.ok("owner", json!({"op":"confirm","req":"r","revision":rev}));
    for ac in ["ios", "key"] {
        let mut cmd = assess(ac, true);
        cmd["revision"] = json!(rev);
        w.ok("agent", cmd);
    }
    w.ok("owner", json!({"op":"define_verification_scope","req":"r","revision":rev,"subject_keys":["app","backend","infra","environment"],"scope":"iOS access and provider settings","reason":"Define the tested components"}));
    let mut cmd = plan("custom-fields");
    cmd["revision"] = json!(rev);
    w.ok("runner", cmd);
    let mut cmd = result(
        "custom-fields",
        "[ios] paid access",
        "pass",
        vec!["access retained"],
    );
    cmd["revision"] = json!(rev);
    w.ok("runner", cmd);
    let mut cmd = result(
        "custom-fields",
        "key-permissions",
        "pass",
        vec!["read", "verify"],
    );
    cmd["revision"] = json!(rev);
    w.ok("owner", cmd);
    w.ok(
        "runner",
        json!({"op":"finish","req":"r","revision":rev,"run":"custom-fields"}),
    );
    let current = w.state.objects["requirement:r"].current();
    assert_eq!(current.data["rationale"], data["rationale"]);
    assert_eq!(current.data["labels"], data["labels"]);
    assert!(joppa::checks::summary(w.state.requirements["r"].revisions.last().unwrap())["ready_for_acceptance"].as_bool().unwrap());
    assert_eq!(
        serde_json::to_value(&w.state.objects["requirement:r"].revisions[..baseline.len()])
            .unwrap(),
        serde_json::to_value(baseline).unwrap()
    );
    let mut forged = data.clone();
    forged["confirmed"] = json!(true);
    let current_revision = w.state.objects["requirement:r"].current().revision;
    w.no("agent",json!({"op":"save_object","kind":"requirement","id":"r","revision":current_revision,"parent":"capability:c","data":forged,"reason":"Inject process evidence"}));
    data.as_object_mut().unwrap().remove("rationale");
    data.as_object_mut().unwrap().remove("labels");
    save(&mut w, "requirement:r", Some("capability:c"), data, vec![]);
    assert!(
        w.state.objects["requirement:r"]
            .current()
            .data
            .get("rationale")
            .is_none()
    );
    assert!(
        w.state.objects["requirement:r"]
            .current()
            .data
            .get("labels")
            .is_none()
    );
    assert_eq!(
        w.state.requirements["r"].revisions[(rev - 1) as usize].runs["custom-fields"]
            .results
            .len(),
        2
    );
}

fn archive_world() -> World {
    let mut w = World::new();
    let mut types = objects::defaults();
    let mut group = types["domain"].clone();
    group.title = "Initiative".into();
    group.parents = vec!["initiative".into()];
    types.insert("initiative".into(), group);
    w.ok("agent", json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Configure group nesting"}));
    w
}

#[test]
fn archive_configured_documents_preserves_history_and_child_guards() {
    let mut w = archive_world();
    let mut configured = objects::schema(&w.state);
    let mut input = configured.types["note"].clone();
    input.title = "Incoming information".into();
    input.parents = vec!["incoming".into()];
    configured.types.insert("incoming".into(), input);
    w.ok("agent", json!({"op":"set_tree_schema","revision":configured.revision,"types":configured.types,"reason":"Use a custom document type"}));
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
    w.no("agent", json!({"op":"archive_object","target":"incoming:parent","revision":1,"reason":"Reject a live child"}));
    w.ok("agent", json!({"op":"archive_object","target":"incoming:child","revision":1,"reason":"Archive the child"}));
    w.ok("agent", json!({"op":"archive_object","target":"incoming:parent","revision":1,"reason":"Archive the review"}));
    assert!(!w.state.objects["incoming:parent"].current().present);
    assert_eq!(
        serde_json::to_value(&w.state.objects["incoming:parent"].revisions[0]).unwrap(),
        original
    );
    w.no("agent", json!({"op":"restore_object","target":"incoming:child","revision":2,"reason":"Reject an absent parent"}));
    w.no("agent", json!({"op":"restore_object","target":"incoming:parent","revision":1,"reason":"Reject a stale revision"}));
    w.ok("agent", json!({"op":"restore_object","target":"incoming:parent","revision":2,"reason":"Restore the review"}));
    assert_eq!(
        w.state.objects["incoming:parent"].current().data["body"],
        "Keep history"
    );
    assert_eq!(w.state.objects["incoming:parent"].current().links.len(), 1);
    w.ok("agent", json!({"op":"restore_object","target":"incoming:child","revision":2,"reason":"Restore the child"}));
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
    w.no("agent", json!({"op":"archive_object","target":"initiative:i","revision":0,"reason":"Archive group"}));
    w.no("legal", json!({"op":"archive_object","target":"initiative:i","revision":1,"reason":"Archive group"}));
    w.ok("agent", json!({"op":"archive_object","target":"initiative:i","revision":1,"reason":"Archive group"}));
    assert!(!w.state.objects["initiative:i"].current().present);
    assert_eq!(w.state.objects["initiative:i"].current().revision, 2);
    w.no("agent", json!({"op":"archive_object","target":"initiative:i","revision":2,"reason":"Archive again"}));
    w.no("agent", json!({"op":"restore_object","target":"initiative:i","revision":1,"reason":"Stale restore"}));
    w.ok("agent", json!({"op":"restore_object","target":"initiative:i","revision":2,"reason":"Restore group"}));
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
    w.no("agent", json!({"op":"archive_object","target":"initiative:parent","revision":1,"reason":"Archive parent"}));
    w.no("agent", json!({"op":"archive_object","target":"requirement:r","revision":1,"reason":"Archive a specification"}));
    w.ok("agent", json!({"op":"archive_object","target":"initiative:child","revision":1,"reason":"Archive child"}));
    w.ok("agent", json!({"op":"archive_object","target":"initiative:parent","revision":1,"reason":"Archive empty parent"}));
    w.no("agent", json!({"op":"restore_object","target":"initiative:child","revision":2,"reason":"Restore before parent"}));
    w.ok("agent", json!({"op":"restore_object","target":"initiative:parent","revision":2,"reason":"Restore parent"}));
    w.ok("agent", json!({"op":"restore_object","target":"initiative:child","revision":2,"reason":"Restore child"}));
}

#[test]
fn archive_legacy_group_does_not_resync() {
    let mut w = World::new();
    w.ok(
        "owner",
        json!({"op":"domain","id":"unused","title":"Unused","owner":"owner"}),
    );
    w.ok("agent", json!({"op":"archive_object","target":"domain:unused","revision":1,"reason":"Archive empty group"}));
    w.ok(
        "owner",
        json!({"op":"domain","id":"other","title":"Other","owner":"owner"}),
    );
    assert!(!w.state.objects["domain:unused"].current().present);
    assert_eq!(w.state.objects["domain:unused"].current().revision, 2);
    w.ok("agent", json!({"op":"restore_object","target":"domain:unused","revision":2,"reason":"Restore group"}));
    w.ok(
        "owner",
        json!({"op":"domain","id":"another","title":"Another","owner":"owner"}),
    );
    assert!(w.state.objects["domain:unused"].current().present);
    assert_eq!(w.state.objects["domain:unused"].current().revision, 3);
}

#[test]
fn restore_legacy_group_reflects_edits() {
    let mut w = World::new();
    w.ok(
        "owner",
        json!({"op":"domain","id":"unused","title":"Unused","owner":"owner"}),
    );
    w.ok("agent", json!({"op":"archive_object","target":"domain:unused","revision":1,"reason":"Archive group"}));
    w.no("owner", json!({"op":"edit_domain","id":"unused","title":"Hidden edit","owner":"owner","reason":"Edit an archived group"}));
    w.ok("agent", json!({"op":"restore_object","target":"domain:unused","revision":2,"reason":"Restore group"}));
    w.ok("owner", json!({"op":"edit_domain","id":"unused","title":"Changed","owner":"owner","reason":"Edit restored group"}));
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
    w.ok("agent", json!({"op":"archive_object","target":"initiative:i","revision":1,"reason":"Archive group"}));
    w.ok("agent", json!({"op":"restore_object","target":"initiative:i","revision":2,"reason":"Restore group"}));
    w.ok(
        "agent",
        json!({"op":"note","target":"domain:d","id":"unrelated","text":"Unrelated context"}),
    );
    assert_eq!(w.state.objects["initiative:i"].current().data, data);
    assert_eq!(w.state.objects["initiative:i"].current().revision, 3);
    assert_eq!(w.state.document_overrides, overrides);
    assert!(w.state.archived_objects.is_empty());
}
