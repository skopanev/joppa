mod common;

use common::World;
use joppa::commands::{self, Command};
use serde_json::json;

fn title(length: usize) -> String {
    "a".repeat(length)
}

fn refused_for_length(w: &mut World, command: serde_json::Value) {
    let error = w.apply("owner", command).unwrap_err().to_string();
    assert!(error.contains("title is at most 64 characters"), "{error}");
}

#[test]
fn new_titles_fit_the_type_preview_length() {
    let mut w = World::new();
    refused_for_length(&mut w, json!({"op":"capability","id":"long","title":title(65),"domain":"d"}));
    w.ok("owner", json!({"op":"capability","id":"short","title":title(64),"domain":"d"}));
    refused_for_length(&mut w, json!({"op":"domain","id":"long","title":title(65),"authority":"owner"}));
    let mut revise = common::revision(1);
    revise["acs"][0]["text"] = json!(title(65));
    refused_for_length(&mut w, revise);
}

#[test]
fn old_journals_with_long_titles_still_replay() {
    let mut w = World::new();
    let command: Command = serde_json::from_value(
        json!({"op":"capability","id":"old","title":title(120),"domain":"d"}),
    )
    .unwrap();
    commands::apply_replay(&mut w.state, "owner", &command, w.position + 1, "2026-01-01T00:00:00Z")
        .expect("replay does not check title length");
}
