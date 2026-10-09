//! Specification rules that the server already enforces, one test per criterion.
//! Each test name says what it proves; the criterion lists it by this address.
mod common;

use common::World;
use serde_json::{Value, json};

fn refused(w: &mut World, actor: &str, command: Value, error: &str) {
    let e = w
        .apply(actor, command.clone())
        .expect_err(&format!("{command} must be refused"));
    assert!(e.to_string().contains(error), "{command}: {e}");
}
fn note(id: &str, revision: u32, links: Value) -> Value {
    json!({"op":"save_object","kind":"note","id":id,"revision":revision,"parent":null,
           "data":{"title":id,"owner":"owner","body":"Signal"},"links":links,"reason":"Update"})
}
fn schema_types(w: &World) -> Value {
    let mut types = joppa::objects::schema(&w.state).types;
    types.remove("intake");
    types.remove("point");
    serde_json::to_value(types).unwrap()
}

#[test]
fn a_save_on_a_stale_object_revision_is_refused() {
    // edit-makes-revision-ac2
    let mut w = World::new();
    w.ok("agent", note("a", 0, json!([])));
    w.ok("agent", note("a", 1, json!([])));
    refused(
        &mut w,
        "agent",
        note("a", 1, json!([])),
        "stale object revision; read the object again",
    );
    assert_eq!(w.state.objects["note:a"].current().revision, 2);
}

#[test]
fn a_link_with_an_unknown_relation_is_refused() {
    // typed-links-ac2
    let mut w = World::new();
    w.ok("agent", note("a", 0, json!([])));
    w.ok("agent", note("b", 0, json!([])));
    refused(
        &mut w,
        "agent",
        note("b", 1, json!([{"target":"note:a","relation":"likes"}])),
        "unknown link relation",
    );
}

#[test]
fn a_blocking_dependency_cycle_is_refused() {
    // blocking-pins-revision-ac2
    let mut w = World::new();
    w.ok("agent", note("a", 0, json!([])));
    w.ok("agent", note("b", 0, json!([])));
    w.ok(
        "agent",
        note(
            "a",
            1,
            json!([{"target":"note:b","relation":"depends_on","revision":1,"blocking":true}]),
        ),
    );
    refused(
        &mut w,
        "agent",
        note(
            "b",
            1,
            json!([{"target":"note:a","relation":"depends_on","revision":2,"blocking":true}]),
        ),
        "dependency cycle",
    );
}

#[test]
fn a_link_outside_the_workspace_is_refused() {
    // links-in-workspace-ac1, workspaces-isolated-ac2
    let mut w = World::new();
    w.ok("agent", note("a", 0, json!([])));
    refused(
        &mut w,
        "agent",
        note(
            "a",
            1,
            json!([{"target":"note:only-in-workspace-b","relation":"related"}]),
        ),
        "unknown linked object in this workspace",
    );
}

#[test]
fn an_agent_cannot_confirm_the_owner_wording() {
    // draft-binds-after-confirm-ac1, owner-confirms-wording-ac2
    let mut w = World::new();
    refused(
        &mut w,
        "agent",
        json!({"op":"confirm","req":"r","revision":1}),
        "REQ owner confirms its formulation",
    );
    assert!(!w.state.requirements["r"].revisions[0].confirmed);
}

#[test]
fn an_object_with_live_children_cannot_be_archived() {
    // removal-is-archiving-ac5
    let mut w = World::new();
    refused(
        &mut w,
        "owner",
        json!({"op":"archive_object","target":"capability:c","revision":1,"reason":"Remove"}),
        "cannot archive an object with active children",
    );
}

#[test]
fn a_human_who_is_not_the_owner_cannot_archive() {
    // removal-is-archiving-ac3
    let mut w = World::new();
    w.ok("agent", note("a", 0, json!([])));
    refused(
        &mut w,
        "legal",
        json!({"op":"archive_object","target":"note:a","revision":1,"reason":"Remove"}),
        "workspace owner or agent required",
    );
    assert!(w.state.objects["note:a"].current().present);
}

#[test]
fn a_requirement_without_an_owner_is_refused() {
    // req-has-criteria-ac1
    let mut w = World::new();
    refused(
        &mut w,
        "agent",
        json!({"op":"revise","req":"r2","expected_revision":0,"capability":"c","title":"T","body":"b","owner":"","consumers":["customer"],"reason":"Draft","acs":[]}),
        "a human member is required",
    );
}

#[test]
fn a_schema_save_on_a_stale_revision_is_refused() {
    // tree-schema-revision-ac2
    let mut w = World::new();
    let types = schema_types(&w);
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Configure"}),
    );
    let e = w
        .apply(
            "owner",
            json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Stale"}),
        )
        .unwrap_err();
    println!("stale schema error: {e}");
    assert!(e.to_string().contains("stale"), "{e}");
    assert_eq!(joppa::objects::schema(&w.state).revision, 1);
}

#[test]
fn a_human_who_is_not_the_owner_cannot_change_the_tree() {
    // tree-owner-sets-types-ac2
    let mut w = World::new();
    let types = schema_types(&w);
    let e = w
        .apply(
            "legal",
            json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Change"}),
        )
        .unwrap_err();
    println!("human schema error: {e}");
    assert_eq!(joppa::objects::schema(&w.state).revision, 0);
}

#[test]
fn an_agent_member_can_change_the_tree() {
    // tree-owner-sets-types-ac3
    let mut w = World::new();
    let types = schema_types(&w);
    w.ok(
        "agent",
        json!({"op":"set_tree_schema","revision":0,"types":types,"reason":"Configure"}),
    );
    assert_eq!(joppa::objects::schema(&w.state).revision, 1);
}
