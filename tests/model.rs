mod common;
use common::*;
use serde_json::json;

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
