//! Owner wording and agent work rules that the server already enforces, one test per criterion.
mod common;

use common::*;
use serde_json::{Value, json};

fn refused(w: &mut World, actor: &str, command: Value, error: &str) {
    let e = w
        .apply(actor, command.clone())
        .expect_err(&format!("{command} must be refused"));
    assert!(e.to_string().contains(error), "{command}: {e}");
}
fn assess_on(revision: u32, ac: &str, ready: bool, recipient: Option<&str>) -> Value {
    json!({"op":"assess","req":"r","revision":revision,"ac":ac,"assessment":{"ready":ready,"reason":if ready {"Known scope"} else {"Which permissions?"},"recipient":recipient,"decision":null}})
}
fn task(id: &str, ac: &str) -> Value {
    json!({"op":"task","req":"r","revision":1,"id":id,"title":id,"acs":[ac],"projects":[],"purpose":"implementation","contract":"Implement and provide evidence"})
}

#[test]
fn the_owner_confirms_the_wording_and_a_member_cannot() {
    // owner-confirms-wording-ac1, ac3; draft-binds-after-confirm-ac2
    let mut w = World::new();
    refused(
        &mut w,
        "legal",
        json!({"op":"confirm","req":"r","revision":1}),
        "REQ owner confirms its formulation",
    );
    assert!(!w.state.requirements["r"].revisions[0].confirmed);
    w.ok("owner", json!({"op":"confirm","req":"r","revision":1}));
    assert!(w.state.requirements["r"].revisions[0].confirmed);
}

#[test]
fn an_agent_edit_of_a_confirmed_requirement_makes_a_new_draft() {
    // draft-binds-after-confirm-ac3
    let mut w = World::new();
    w.ok("owner", json!({"op":"confirm","req":"r","revision":1}));
    w.ok("agent", revision(1));
    let last = w.state.requirements["r"].revisions.last().unwrap();
    assert_eq!((last.number, last.confirmed), (2, false));
}

#[test]
fn an_agent_records_readiness_per_criterion_with_a_reason() {
    // readiness-per-criterion-ac1
    let mut w = World::new();
    w.ok("agent", assess_on(1, "ios", true, None));
    let acs = &w.state.requirements["r"].revisions[0].acs;
    let ios = acs.iter().find(|a| a.id == "ios").unwrap();
    let key = acs.iter().find(|a| a.id == "key").unwrap();
    assert_eq!(
        ios.assessment
            .as_ref()
            .map(|a| (a.ready, a.reason.as_str())),
        Some((true, "Known scope"))
    );
    assert!(key.assessment.is_none());
}

#[test]
fn readiness_on_an_unsaved_revision_is_refused() {
    // readiness-per-criterion-ac2
    let mut w = World::new();
    refused(
        &mut w,
        "agent",
        assess_on(2, "ios", true, None),
        "stale revision",
    );
}

#[test]
fn a_not_ready_assessment_names_the_gap_and_a_person() {
    // not-ready-names-gap-ac1, ac2
    let mut w = World::new();
    refused(
        &mut w,
        "agent",
        assess_on(1, "key", false, None),
        "waiting needs a recipient",
    );
    w.ok("agent", assess_on(1, "key", false, Some("legal")));
    let key = w.state.requirements["r"].revisions[0]
        .acs
        .iter()
        .find(|a| a.id == "key")
        .unwrap()
        .assessment
        .clone()
        .unwrap();
    assert_eq!(
        (key.ready, key.reason.as_str(), key.owner.as_deref()),
        (false, "Which permissions?", Some("legal"))
    );
}

#[test]
fn a_new_revision_resets_readiness() {
    // readiness-resets-ac1
    let mut w = World::new();
    w.ok("agent", assess_on(1, "ios", true, None));
    w.ok("owner", revision(1));
    let last = w.state.requirements["r"].revisions.last().unwrap();
    assert!(last.acs.iter().all(|a| a.assessment.is_none()));
}

#[test]
fn a_task_on_a_not_ready_criterion_cannot_be_claimed() {
    // no-work-before-ready-ac1
    let mut w = World::new();
    w.ok("owner", task("t1", "key"));
    refused(
        &mut w,
        "agent",
        json!({"op":"claim_task","req":"r","revision":1,"id":"t1"}),
        "AC is not ready for work",
    );
}
