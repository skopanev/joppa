//! The journal side of remote links: the shape checks and a replay of the stored links.
mod common;

use common::World;
use joppa::commands::{self, Command};
use serde_json::{Value, json};

fn link(target: &str, relation: &str, blocking: bool) -> Value {
    json!({"workspace":"backend","target":target,"relation":relation,"blocking":blocking})
}

fn set(links: Value) -> Value {
    json!({"op":"set_remote_links","target":"domain:d","links":links,"reason":"Use the API contract"})
}

#[test]
fn bad_remote_links_are_refused_and_change_nothing() {
    let mut w = World::new();
    for (links, error) in [
        (
            json!([link("requirement:api", "related", true)]),
            "blocking links must be dependencies",
        ),
        (
            json!([link("api", "related", false)]),
            "a remote link target is an address kind:id",
        ),
        (
            json!([link("requirement:api", "uses", false)]),
            "unknown link relation",
        ),
        (
            json!([
                link("requirement:api", "related", false),
                link("requirement:api", "related", false)
            ]),
            "duplicate remote link",
        ),
    ] {
        let e = w
            .apply("agent", set(links))
            .expect_err("a bad link must be refused");
        assert!(e.to_string().contains(error), "{e}");
    }
    let e = w
        .apply(
            "agent",
            json!({"op":"set_remote_links","target":"domain:none","links":[],"reason":"No object"}),
        )
        .expect_err("an unknown source must be refused");
    assert!(e.to_string().contains("unknown object"), "{e}");
    assert!(w.state.remote_links.is_empty());
}

#[test]
fn stored_remote_links_replay_to_the_same_state_and_an_empty_list_removes_them() {
    let mut w = World::new();
    let first = set(json!([link("requirement:api", "depends_on", true)]));
    w.ok("agent", first.clone());
    let saved = serde_json::to_value(&w.state.remote_links).unwrap();
    assert_eq!(saved["domain:d"][0]["target"], "requirement:api");
    // The journal stores the serialized command, so a replay reads that form.
    let cmd: Command = serde_json::from_value(first).unwrap();
    let stored: Command = serde_json::from_value(serde_json::to_value(&cmd).unwrap()).unwrap();
    let mut replay = World::new();
    commands::apply_replay(
        &mut replay.state,
        "agent",
        &stored,
        replay.position + 1,
        "now",
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&replay.state.remote_links).unwrap(),
        saved
    );
    w.ok("agent", set(json!([])));
    assert!(w.state.remote_links.is_empty());
}
