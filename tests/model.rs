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
fn independent_acs_and_cross_repository_tasks_do_not_fake_green() {
    let mut w = World::new();
    w.ok("agent", assess("ios", true));
    w.ok("agent", assess("key", false));
    let mut task = json!({"op":"task","req":"r","revision":1,"id":"t","title":"Implement iOS access","acs":["ios"],"projects":["app","backend","infra"],"purpose":"implementation","assignee":"agent"});
    w.ok("agent", task.clone());
    w.ok("agent",json!({"op":"complete_task","req":"r","revision":1,"id":"t","evidence":"three changes merged"}));
    assert_eq!(w.summary(1)["acs"]["ios"]["verification"], "n/a");
    task["id"] = json!("t2");
    task["acs"] = json!(["key"]);
    w.no("agent", task.clone());
    task["purpose"] = json!("decision");
    w.ok("agent", task);
    assert_eq!(w.state.requirements["r"].revisions[0].acs.len(), 2);
}
#[test]
fn unassigned_task_can_be_claimed_only_when_its_ac_is_ready() {
    let mut w = World::new();
    let task = json!({"op":"task","req":"r","revision":1,"id":"free","title":"Implement iOS access","acs":["ios"],"projects":["app"],"purpose":"implementation","contract":"Change app access flow and attach merged build evidence"});
    w.ok("agent", task);
    let claim = json!({"op":"claim_task","req":"r","revision":1,"id":"free"});
    w.no("agent", claim.clone());
    w.no(
        "agent",
        json!({"op":"complete_task","req":"r","revision":1,"id":"free","evidence":"merge"}),
    );
    w.ok("agent", assess("ios", true));
    w.ok("agent", claim.clone());
    assert_eq!(
        w.state.requirements["r"].revisions[0].tasks["free"]
            .assignee
            .as_deref(),
        Some("agent")
    );
    w.no("owner", claim);
    w.ok("agent", json!({"op":"release_task","req":"r","revision":1,"id":"free","reason":"Handing work back to the queue"}));
    w.ok(
        "owner",
        json!({"op":"claim_task","req":"r","revision":1,"id":"free"}),
    );
    w.ok(
        "owner",
        json!({"op":"complete_task","req":"r","revision":1,"id":"free","evidence":"merge abc"}),
    );
    assert_eq!(w.summary(1)["acs"]["ios"]["verification"], "n/a");
}
#[test]
fn creator_can_correct_unclaimed_task_without_revising_req() {
    let mut w = World::new();
    w.ok("agent",json!({"op":"task","req":"r","revision":1,"id":"wrong","title":"Build flow","acs":["ios"],"projects":["infra"],"purpose":"implementation","contract":"Build the iOS flow"}));
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
        w.ok("agent", json!({"op":"task","req":"r","revision":1,"id":id,"title":"Implement access","acs":["ios"],"projects":["app"],"purpose":"implementation","assignee":"agent"}));
    }
    w.ok("agent",json!({"op":"complete_task","req":"r","revision":1,"id":"finished","evidence":"merged build abc"}));
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
    let complete = json!({"op":"complete_task","req":"r","revision":2,"id":"ongoing","evidence":"merged build def"});
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
    w.ok("agent", json!({"op":"task","req":"r","revision":1,"id":"old","title":"Old implementation","acs":["ios"],"projects":["app"],"purpose":"implementation","assignee":"agent"}));
    w.ok("agent", revision(1)); // Historical revise event: no task-carry marker.
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
        json!({"op":"complete_task","req":"r","revision":3,"id":"old","evidence":"merged"}),
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
fn removed_check_across_revision_needs_explicit_owner_review() {
    let mut w = World::new();
    w.ready();
    w.run("first", "pass");
    let mut next = revision(1);
    next["coverage_policy"] = json!(true);
    w.ok("agent", next);
    w.ok("owner", json!({"op":"confirm","req":"r","revision":2}));
    for ac in ["ios", "key"] {
        let mut a = assess(ac, true);
        a["revision"] = json!(2);
        w.ok("agent", a);
    }
    w.ok("owner",json!({"op":"define_verification_scope","req":"r","revision":2,"subject_keys":["app","backend","infra","environment"],"scope":"iOS access and provider settings","reason":"Full access and key scope"}));
    let mut second = plan("second");
    second["revision"] = json!(2);
    second["checks"][0]["id"] = json!("[ios] replacement check");
    w.ok("runner", second);
    assert_eq!(
        w.summary(2)["coverage_change"][0]["id"],
        "[ios] paid access"
    );
    assert_eq!(w.summary(2)["coverage_reviewed"], false);
    let mut ios = result(
        "second",
        "[ios] replacement check",
        "pass",
        vec!["access retained"],
    );
    ios["revision"] = json!(2);
    w.ok("runner", ios);
    let mut key = result("second", "key-permissions", "pass", vec!["read", "verify"]);
    key["revision"] = json!(2);
    w.ok("owner", key);
    w.ok(
        "runner",
        json!({"op":"finish","req":"r","revision":2,"run":"second"}),
    );
    assert_eq!(w.summary(2)["acs"]["ios"]["verification"], "pass");
    assert_eq!(w.summary(2)["ready_for_acceptance"], false);
    let accept = json!({"op":"accept","req":"r","revision":2,"run":"second"});
    w.no("owner", accept.clone());
    let review = json!({"op":"review_coverage","req":"r","revision":2,"run":"second","reason":"Old check was replaced with a correct contract check"});
    w.no("agent", review.clone());
    w.ok("owner", review);
    assert_eq!(w.summary(2)["ready_for_acceptance"], true);
    w.ok("owner", accept);
}
#[test]
fn coverage_compares_to_last_prior_plan_even_when_intermediate_revision_has_none() {
    let mut w = World::new();
    w.ready();
    w.run("first", "pass");
    w.ok("agent", revision(1));
    let mut third = revision(2);
    third["coverage_policy"] = json!(true);
    w.ok("agent", third);
    w.ok("owner", json!({"op":"define_verification_scope","req":"r","revision":3,"subject_keys":["app","backend","infra","environment"],"scope":"iOS access and provider settings","reason":"Preserve full subject"}));
    let mut new_plan = plan("third");
    new_plan["revision"] = json!(3);
    new_plan["checks"][0]["id"] = json!("[ios] replacement check");
    w.ok("runner", new_plan);
    assert_eq!(
        w.summary(3)["coverage_change"][0]["id"],
        "[ios] paid access"
    );
}
#[test]
fn owner_scope_contract_prevents_self_nominated_subject_coverage() {
    let mut w = World::new();
    w.ok("agent", assess("ios", true));
    w.ok("agent",json!({"op":"task","req":"r","revision":1,"id":"api","title":"Implement access","acs":["ios"],"projects":["app","backend"],"purpose":"implementation","assignee":"agent"}));
    let mut next = revision(1);
    next["coverage_policy"] = json!(true);
    next["carry_tasks"] = json!(true);
    w.ok("agent", next);
    let mut plan2 = plan("second");
    plan2["revision"] = json!(2);
    w.no("runner", plan2.clone()); // Owner has not agreed the subject boundary.
    let define = json!({"op":"define_verification_scope","req":"r","revision":2,"subject_keys":["app","backend","environment"],"scope":"iOS access and provider settings","reason":"These components affect both ACs"});
    w.no("agent", define.clone());
    w.ok("owner", define);
    w.no("runner", plan2.clone()); // The extra infra key was not agreed.
    plan2["subject"].as_object_mut().unwrap().remove("infra");
    let mut missing = plan2.clone();
    missing["subject"]
        .as_object_mut()
        .unwrap()
        .remove("backend");
    w.no("runner", missing);
    let mut environment_only = plan2.clone();
    environment_only["subject"]["backend"] = json!("production");
    w.no("runner", environment_only);
    let mut narrowed = plan2.clone();
    narrowed["scope"] = json!("Only the app");
    w.no("runner", narrowed);
    w.ok("runner", plan2);
    w.no("owner",json!({"op":"define_verification_scope","req":"r","revision":2,"subject_keys":["app"],"scope":"Only app","reason":"Narrowed after run"}));
}
#[test]
fn revision_resets_every_ac_and_old_evidence_and_acceptance_survive() {
    let mut w = World::new();
    w.ready();
    w.run("first", "pass");
    w.ok(
        "owner",
        json!({"op":"accept","req":"r","revision":1,"run":"first"}),
    );
    assert_eq!(w.summary(1)["accepted"], true);
    w.ok("runner", plan("late"));
    w.ok("agent", revision(1));
    assert_eq!(w.summary(2)["acs"]["ios"]["verification"], "n/a");
    assert_eq!(w.summary(2)["accepted"], false);
    assert_eq!(w.summary(1)["accepted"], true);
    w.ok(
        "runner",
        result("late", "[ios] paid access", "pass", vec!["access retained"]),
    );
    w.ok(
        "owner",
        result("late", "key-permissions", "pass", vec!["read", "verify"]),
    );
    w.ok(
        "runner",
        json!({"op":"finish","req":"r","revision":1,"run":"late"}),
    );
    assert_eq!(w.summary(2)["acs"]["key"]["verification"], "n/a");
    w.no(
        "owner",
        json!({"op":"accept","req":"r","revision":1,"run":"late"}),
    );
    w.no("agent", revision(1));
}
#[test]
fn full_sealed_plan_and_manual_evidence_are_required() {
    let mut w = World::new();
    w.ready();
    let mut p = plan("empty");
    p["checks"] = json!([]);
    w.no("runner", p);
    let mut p = plan("duplicate");
    p["checks"][1]["id"] = json!("[ios] paid access");
    w.no("runner", p);
    w.ok("runner", plan("one"));
    w.no(
        "runner",
        result("one", "key-permissions", "pass", vec!["read", "verify"]),
    );
    w.no(
        "owner",
        result(
            "one",
            "key-permissions",
            "pass",
            vec!["read", "verify", "delete"],
        ),
    );
    w.no("owner", result("one", "key-permissions", "pass", vec![]));
    w.ok(
        "runner",
        result("one", "[ios] paid access", "pass", vec!["access retained"]),
    );
    w.no(
        "runner",
        result("one", "[ios] paid access", "pass", vec!["access retained"]),
    );
    w.ok(
        "runner",
        json!({"op":"finish","req":"r","revision":1,"run":"one"}),
    );
    assert_eq!(w.summary(1)["ready_for_acceptance"], false);
    let mut p = plan("shrink");
    p["checks"][0]["id"] = json!("[ios] replacement");
    w.no("runner", p);
    let mut p = plan("incomplete");
    p["discovery_complete"] = json!(false);
    w.ok("runner", p);
    w.ok(
        "runner",
        result(
            "incomplete",
            "[ios] paid access",
            "pass",
            vec!["access retained"],
        ),
    );
    w.ok(
        "owner",
        result(
            "incomplete",
            "key-permissions",
            "pass",
            vec!["read", "verify"],
        ),
    );
    w.ok(
        "runner",
        json!({"op":"finish","req":"r","revision":1,"run":"incomplete"}),
    );
    assert_eq!(w.summary(1)["ready_for_acceptance"], false);
}
#[test]
fn later_failure_and_late_old_success_cannot_be_hidden() {
    let mut w = World::new();
    w.ready();
    w.run("green", "pass");
    w.ok("runner", plan("delayed"));
    w.run("red", "fail");
    w.no(
        "owner",
        json!({"op":"accept","req":"r","revision":1,"run":"green"}),
    );
    w.ok(
        "runner",
        result(
            "delayed",
            "[ios] paid access",
            "pass",
            vec!["access retained"],
        ),
    );
    w.ok(
        "owner",
        result("delayed", "key-permissions", "pass", vec!["read", "verify"]),
    );
    w.ok(
        "runner",
        json!({"op":"finish","req":"r","revision":1,"run":"delayed"}),
    );
    assert_eq!(w.summary(1)["run"], "red");
    assert_eq!(w.summary(1)["acs"]["ios"]["verification"], "fail");
    w.run("fixed", "pass");
    w.no(
        "agent",
        json!({"op":"accept","req":"r","revision":1,"run":"fixed"}),
    );
    w.ok(
        "owner",
        json!({"op":"accept","req":"r","revision":1,"run":"fixed"}),
    );
}
#[test]
fn late_failure_from_older_started_run_does_not_replace_newer_pass() {
    let mut w = World::new();
    w.ready();
    w.ok("runner", plan("older"));
    w.run("newer", "pass");
    w.ok(
        "runner",
        result("older", "[ios] paid access", "fail", vec!["access lost"]),
    );
    w.ok(
        "owner",
        result("older", "key-permissions", "pass", vec!["read", "verify"]),
    );
    w.ok(
        "runner",
        json!({"op":"finish","req":"r","revision":1,"run":"older"}),
    );
    assert_eq!(w.summary(1)["run"], "newer");
    assert_eq!(w.summary(1)["acs"]["ios"]["verification"], "pass");
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
fn ordinary_comment_does_not_invalidate_but_confirmed_issue_requires_new_run() {
    let mut w = World::new();
    w.ready();
    w.run("first", "pass");
    let c = json!({"op":"comment","req":"r","revision":1,"id":"bug","text":"Access disappears on relogin","kind":"comment","reply_to":null,"ac":"ios"});
    w.ok("agent", c);
    assert_eq!(w.summary(1)["ready_for_acceptance"], true);
    let issue = json!({"op":"issue","req":"r","revision":1,"id":"bug","ac":"ios","comment":"bug"});
    w.no("agent", issue.clone());
    w.ok("owner", issue);
    assert_eq!(w.summary(1)["ready_for_acceptance"], false);
    w.ok("runner", plan("before-fix"));
    w.ok(
        "owner",
        json!({"op":"resolve","req":"r","revision":1,"id":"bug","comment":"bug"}),
    );
    w.ok(
        "runner",
        result(
            "before-fix",
            "[ios] paid access",
            "pass",
            vec!["access retained"],
        ),
    );
    w.ok(
        "owner",
        result(
            "before-fix",
            "key-permissions",
            "pass",
            vec!["read", "verify"],
        ),
    );
    w.ok(
        "runner",
        json!({"op":"finish","req":"r","revision":1,"run":"before-fix"}),
    );
    assert_eq!(w.summary(1)["ready_for_acceptance"], false);
    w.run("fixed", "pass");
    assert_eq!(w.summary(1)["ready_for_acceptance"], true);
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
fn intake_is_verbatim_and_routes_require_real_answers() {
    let mut w = World::new();
    let said = "  it broke\n\nplease help  ";
    w.ok(
        "agent",
        json!({"op":"intake","id":"i","said":said,"author":"customer"}),
    );
    assert_eq!(w.state.intake["i"].said, said);
    w.ok("agent", json!({"op":"split","id":"i","points":[said]}));
    w.ok(
        "agent",
        json!({"op":"sort","id":"i","point":"i.1","kind":"bug"}),
    );
    let route = json!({"op":"route","id":"i","point":"i.1","outcome":"work","reason":"Matches existing behavior","req":"r"});
    w.no("agent", route.clone());
    for field in ["observed", "expected", "where"] {
        w.ok(
            "agent",
            json!({"op":"answer","id":"i","point":"i.1","field":field,"value":"literal answer"}),
        );
    }
    w.ok("agent", route);
    assert_eq!(w.state.requirements.len(), 1);
    w.no(
        "agent",
        json!({"op":"split","id":"i","points":["replaced"]}),
    );
}
#[test]
fn withdrawn_decision_blocks_only_affected_work_and_preserves_comment() {
    let mut w = World::new();
    w.ok("legal",json!({"op":"comment","req":"r","revision":1,"id":"law","text":"Agreed","kind":"decision","ac":"key"}));
    let mut a = assess("key", true);
    a["assessment"]["decision"] = json!("law");
    w.ok("agent", a.clone());
    w.ok("agent", assess("ios", true));
    let withdraw = json!({"op":"withdraw_decision","req":"r","revision":1,"comment":"law","reason":"New condition needs review"});
    w.no("owner", withdraw.clone());
    w.ok("legal", withdraw);
    w.no("agent", a);
    assert_eq!(w.summary(1)["acs"]["ios"]["ready"], true);
    assert_eq!(w.summary(1)["acs"]["key"]["ready"], false);
    assert_eq!(w.state.requirements["r"].comments["law"].text, "Agreed");
}
#[test]
fn skipped_results_and_second_ac_tags_never_turn_green() {
    let mut w = World::new();
    w.ready();
    let mut p = plan("ambiguous");
    p["checks"][0]["id"] = json!("[ios] [key] misleading test");
    w.no("runner", p);
    w.run("skipped", "skip");
    assert_eq!(w.summary(1)["acs"]["ios"]["verification"], "n/a");
}
#[test]
fn unchanged_specialist_decision_can_be_reused_but_evidence_cannot() {
    let mut w = World::new();
    let mut rev = revision(1);
    rev["acs"][1]["decider"] = json!("legal");
    w.ok("agent", rev.clone());
    w.ok("legal",json!({"op":"comment","req":"r","revision":2,"id":"law","text":"Confirmed rights","kind":"decision","ac":"key"}));
    rev["expected_revision"] = json!(2);
    rev["acs"][0]["text"] = json!("On iOS access survives reauthentication");
    w.ok("agent", rev.clone());
    let mut a = assess("key", true);
    a["revision"] = json!(3);
    a["assessment"]["decision"] = json!("law");
    w.ok("agent", a.clone());
    assert_eq!(w.summary(3)["acs"]["key"]["verification"], "n/a");
    rev["expected_revision"] = json!(3);
    rev["acs"][1]["text"] = json!("Also allow delete");
    w.ok("agent", rev);
    a["revision"] = json!(4);
    w.no("agent", a);
}

#[test]
fn tree_order_is_owner_only_scoped_and_does_not_change_requirement_evidence() {
    let mut w = World::new();
    w.ready();
    w.run("verified", "pass");
    w.ok(
        "owner",
        json!({"op":"domain","id":"other","title":"Other","authority":"owner"}),
    );
    w.ok(
        "owner",
        json!({"op":"capability","id":"second","title":"Second","domain":"d"}),
    );
    w.ok(
        "owner",
        json!({"op":"capability","id":"foreign","title":"Foreign","domain":"other"}),
    );
    let mut second = revision(0);
    second["req"] = json!("second");
    second["acs"] = json!([{"id":"second-ac","text":"Separate criterion"}]);
    w.ok("agent", second);
    let before = serde_json::to_value(&w.state).unwrap();
    for (kind, parent, expected, ids) in [
        ("domain", None, vec!["d", "other"], vec!["other", "d"]),
        (
            "capability",
            Some("d"),
            vec!["c", "second"],
            vec!["second", "c"],
        ),
        (
            "requirement",
            Some("c"),
            vec!["r", "second"],
            vec!["second", "r"],
        ),
    ] {
        let command =
            json!({"op":"reorder_tree","kind":kind,"parent":parent,"expected":expected,"ids":ids});
        w.no("agent", command.clone());
        let mut invalid = command.clone();
        invalid["ids"] = json!(["foreign", "foreign"]);
        w.no("owner", invalid);
        w.ok("owner", command.clone());
        w.no("owner", command); // stale order cannot overwrite another move
        assert_eq!(
            joppa::tree_order::ordered_ids(&w.state, kind, parent).unwrap(),
            ids
        );
    }
    let mut after = serde_json::to_value(&w.state).unwrap();
    after["tree_order"] = before["tree_order"].clone();
    assert_eq!(
        after, before,
        "ordering preserves all content, revisions and checks"
    );
    w.ok(
        "owner",
        json!({"op":"capability","id":"added","title":"Added","domain":"d"}),
    );
    assert_eq!(
        joppa::tree_order::ordered_ids(&w.state, "capability", Some("d")).unwrap(),
        vec!["second", "c", "added"]
    );
    assert!(joppa::tree_order::ordered_ids(&w.state, "domain", Some("d")).is_err());
}

#[test]
fn archive_clears_unused_tree_without_losing_ids_or_allowing_hidden_work() {
    let mut w = World::new();
    w.ok(
        "owner",
        json!({"op":"domain","id":"seed","title":"Example","authority":"owner"}),
    );
    w.ok(
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
