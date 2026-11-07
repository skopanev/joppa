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
