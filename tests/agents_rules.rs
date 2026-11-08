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

// ac:acceptance-needs-green
#[test]
fn a_live_acceptance_is_refused_with_the_replacement() {
    let mut w = World::new();
    for command in [
        json!({"op":"accept_commit","target":"requirement:r","revision":1,"commit":"c1c1c1c"}),
        json!({"op":"accept","req":"r","revision":1,"run":"run-1"}),
    ] {
        let op = command["op"].as_str().unwrap().to_owned();
        refused(&mut w, "owner", command, &joppa::removed_ops::refusal(&op));
    }
    assert!(joppa::removed_ops::refusal("accept_commit").contains("as a Source"));
}
