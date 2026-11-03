//! Agent work rules that the server already enforces, one test per criterion.
mod common;

use common::*;
use serde_json::{Value, json};

fn refused(w: &mut World, actor: &str, command: Value, error: &str) {
    let e = w
        .apply(actor, command.clone())
        .expect_err(&format!("{command} must be refused"));
    assert!(e.to_string().contains(error), "{command}: {e}");
}

#[test]
fn an_agent_records_readiness_per_criterion_with_a_reason() {
    // readiness-per-criterion-ac1
    let mut w = World::new();
    w.ok(
        "agent",
        json!({"op":"assess","target":"ac:ios","ready":true,"reason":"Known scope"}),
    );
    let ios = joppa::objects::assessment(&w.state, "ac:ios").unwrap();
    assert_eq!(
        (&ios["state"], &ios["reason"], &ios["actor"]),
        (&json!("ready"), &json!("Known scope"), &json!("agent"))
    );
    assert!(joppa::objects::assessment(&w.state, "ac:key").is_none());
}

#[test]
fn a_member_who_is_not_the_owner_cannot_accept() {
    // who-may-accept-ac1
    let mut w = World::new();
    refused(
        &mut w,
        "legal",
        json!({"op":"accept_commit","target":"requirement:r","revision":1,"commit":"c1c1c1c"}),
        "only the owner of the node accepts",
    );
}
