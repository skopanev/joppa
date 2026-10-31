//! Planning claim guards for two people. P (`planner`) and Q (`other`) are human members.
mod common;
use common::World;
use joppa::{model::Workspace, objects, tasks};
use serde_json::{Value, json};

fn ready() -> Option<Value> {
    Some(json!({"ready":true,"reason":"Known scope"}))
}

/// R1 (`spec:one`) has criteria A and B, and R2 (`spec:two`) has criterion C. A is at
/// revision 8 and depends on `depends_on`. T0 belongs to A.
/// `a` and `b` are the assessments of A and B; None keeps the criterion unassessed.
fn fixture(depends_on: &[&str], a: Option<Value>, b: Option<Value>) -> World {
    let mut w = World {
        state: Workspace::default(),
        position: 0,
    };
    w.ok(
        "owner",
        json!({"op":"workspace","title":"Planning by people"}),
    );
    for actor in ["planner", "other", "worker"] {
        w.ok("owner", json!({"op":"member","id":actor,"role":"human"}));
    }
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":0,"reason":"Configure roles","types":{
            "spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":[]},
            "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
            "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
        }}),
    );
    objects::migrate_schema(&mut w.state, "owner", w.position + 1, "now").unwrap();
    tasks::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    joppa::body_migration::migrate(&mut w.state, "owner", w.position + 1, "now").unwrap();
    objects::migrate_documents(&mut w.state, "owner", w.position + 1, "now").unwrap();
    for (spec, title) in [("spec:one", "One rule"), ("spec:two", "Two rule")] {
        save(
            &mut w,
            spec,
            None,
            json!({"title":title,"body":"Implement the agreed rule","owner":"owner"}),
        );
    }
    save(
        &mut w,
        "criterion:b",
        Some("spec:one"),
        json!({"title":"criterion:b","owner":"owner"}),
    );
    while revision(&w, "criterion:a") < 8 {
        let draft = format!("Draft {}", revision(&w, "criterion:a"));
        save(
            &mut w,
            "criterion:a",
            Some("spec:one"),
            json!({"title":"criterion:a","owner":"owner","body":draft,"depends_on":depends_on}),
        );
    }
    save(
        &mut w,
        "criterion:c",
        Some("spec:two"),
        json!({"title":"criterion:c","owner":"owner"}),
    );
    confirm(
        &mut w,
        "spec:one",
        &[("criterion:a", a), ("criterion:b", b)],
    );
    confirm(&mut w, "spec:two", &[("criterion:c", ready())]);
    save(
        &mut w,
        "work:t0",
        Some("criterion:a"),
        json!({"title":"T0","owner":"owner","body":"Implement this criterion","purpose":"implementation"}),
    );
    for actor in ["planner", "other"] {
        assert_eq!(w.state.members[actor], "human");
    }
    assert!(w.state.planning_claims.is_empty());
    assert_eq!(revision(&w, "criterion:a"), 8);
    w
}
fn revision(w: &World, key: &str) -> u32 {
    w.state.objects.get(key).map_or(0, |o| o.current().revision)
}
fn save(w: &mut World, key: &str, parent: Option<&str>, data: Value) {
    let (kind, id) = key.split_once(':').unwrap();
    let revision = revision(w, key);
    w.ok("owner", json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Set the contract"}));
}
fn confirm(w: &mut World, req: &str, assessments: &[(&str, Option<Value>)]) {
    let revision = w.state.requirements[req].revisions.last().unwrap().number;
    w.ok(
        "owner",
        json!({"op":"confirm","req":req,"revision":revision}),
    );
    for (ac, assessment) in assessments {
        if let Some(assessment) = assessment {
            w.ok("owner", json!({"op":"assess","req":req,"revision":revision,"ac":ac,"assessment":assessment}));
        }
    }
}
fn claim(w: &mut World, actor: &str, ac: &str) {
    let revision = revision(w, ac);
    w.ok(
        actor,
        json!({"op":"claim_planning","ac":ac,"revision":revision}),
    );
}
/// The claim of `actor` fails with `expected`, and the state and position stay the same.
fn refuse(w: &mut World, actor: &str, ac: &str, revision: u32, expected: &str) {
    let before = serde_json::to_value(&w.state).unwrap();
    let position = w.position;
    let cmd = json!({"op":"claim_planning","ac":ac,"revision":revision});
    assert_eq!(w.apply(actor, cmd).unwrap_err().to_string(), expected);
    assert_eq!(w.position, position);
    assert_eq!(serde_json::to_value(&w.state).unwrap(), before);
}
fn t0(w: &World) -> (Value, Value) {
    let p = objects::process(&w.state, "work:t0");
    (p["status"].clone(), p["assignee"].clone())
}

// planning-claim-ac1
#[test]
fn a_person_wins_a_criterion_and_holds_claims_in_several_reqs() {
    let mut w = fixture(&[], ready(), ready());
    let revision = revision(&w, "work:t0");
    w.ok(
        "worker",
        json!({"op":"claim_task","id":"work:t0","revision":revision}),
    );
    let work = t0(&w);
    assert_eq!(work, (json!("in_progress"), json!("worker")));
    claim(&mut w, "planner", "criterion:a");
    let claimed = w.position;
    assert_eq!(
        objects::process(&w.state, "criterion:a")["planning_claim"],
        json!({"actor":"planner","position":claimed})
    );
    // Q loses A, and T0 keeps its status and assignee.
    refuse(
        &mut w,
        "other",
        "criterion:a",
        8,
        "criterion is already claimed for planning",
    );
    assert_eq!(t0(&w), work);
    // P holds A and claims C of R2 in this workspace.
    claim(&mut w, "planner", "criterion:c");
    assert_eq!(
        objects::process(&w.state, "criterion:c")["planning_claim"]["actor"],
        "planner"
    );
    assert_eq!(
        objects::process(&w.state, "criterion:a")["planning_claim"],
        json!({"actor":"planner","position":claimed})
    );
    // P claims ready C in another workspace while P holds A here.
    let mut other_workspace = fixture(&[], ready(), ready());
    claim(&mut other_workspace, "planner", "criterion:c");
    assert_eq!(
        other_workspace.state.planning_claims["criterion:c"].actor,
        "planner"
    );
    assert_eq!(w.state.planning_claims["criterion:a"].actor, "planner");
}

// planning-claim-ac1
#[test]
fn each_claim_guard_refuses_a_person_and_keeps_the_state() {
    const NOT_READY: &str = "criterion is not ready for planning";
    let waiting = Some(json!({"ready":false,"reason":"Unknown input","recipient":"owner"}));
    let mut w = fixture(&[], None, ready());
    refuse(&mut w, "planner", "criterion:a", 8, NOT_READY);
    let mut w = fixture(&[], waiting.clone(), ready());
    refuse(&mut w, "planner", "criterion:a", 8, NOT_READY);
    // A depends on B, and B waits for an input.
    let mut w = fixture(&["criterion:b"], ready(), waiting);
    let a = objects::process(&w.state, "criterion:a");
    assert_eq!(a["assessment"]["state"], "ready");
    assert_eq!(a["readiness_blocker"]["criterion"], "criterion:b");
    refuse(&mut w, "planner", "criterion:a", 8, NOT_READY);
    let mut w = fixture(&[], ready(), ready());
    refuse(
        &mut w,
        "planner",
        "criterion:a",
        7,
        "stale object revision; read the object again",
    );
    refuse(
        &mut w,
        "planner",
        "criterion:missing",
        1,
        "unknown criterion",
    );
    let t0 = revision(&w, "work:t0");
    refuse(
        &mut w,
        "planner",
        "work:t0",
        t0,
        "planning requires a criterion",
    );
    // Each guard failed alone: the same claim with valid conditions succeeds.
    claim(&mut w, "planner", "criterion:a");
    assert_eq!(w.state.planning_claims["criterion:a"].actor, "planner");
}
