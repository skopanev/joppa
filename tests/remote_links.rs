//! The journal side of remote links: the shape checks, the links revision and a replay.
mod common;

use common::World;
use joppa::commands::{self, Command};
use serde_json::{Value, json};

fn link(target: &str, relation: &str, blocking: bool, revision: Option<u32>) -> Value {
    json!({"workspace":"backend","target":target,"relation":relation,"blocking":blocking,"revision":revision})
}

fn set(revision: u32, links: Value) -> Value {
    json!({"op":"set_remote_links","target":"domain:d","revision":revision,"links":links,"reason":"Use the API contract"})
}

#[test]
fn bad_remote_links_are_refused_and_change_nothing() {
    let mut w = World::new();
    for (links, error) in [
        (
            json!([link("requirement:api", "related", true, Some(1))]),
            "blocking links must be dependencies",
        ),
        (
            json!([link("requirement:api", "depends_on", true, None)]),
            "a blocking remote link needs the target revision",
        ),
        (
            json!([link("api", "related", false, None)]),
            "a remote link target is an address kind:id",
        ),
        (
            json!([link("requirement:api", "uses", false, None)]),
            "unknown link relation",
        ),
        (
            json!([
                link("requirement:api", "related", false, None),
                link("requirement:api", "related", false, None)
            ]),
            "duplicate remote link",
        ),
    ] {
        let e = w
            .apply("agent", set(0, links))
            .expect_err("a bad link must be refused");
        assert!(e.to_string().contains(error), "{e}");
    }
    let e = w
        .apply("agent", json!({"op":"set_remote_links","target":"domain:none","revision":0,"links":[],"reason":"No object"}))
        .expect_err("an unknown source must be refused");
    assert!(e.to_string().contains("unknown object"), "{e}");
    assert!(w.state.remote_links.is_empty());
}

#[test]
fn a_stale_links_revision_is_refused_and_each_write_raises_it() {
    let mut w = World::new();
    w.ok(
        "agent",
        set(
            0,
            json!([link("requirement:api", "depends_on", true, Some(2))]),
        ),
    );
    let e = w
        .apply("agent", set(0, json!([])))
        .expect_err("a writer that read revision 0 must read again");
    assert!(e.to_string().contains("stale remote links revision"), "{e}");
    w.ok("agent", set(1, json!([])));
    let stored = &w.state.remote_links["domain:d"];
    assert_eq!(
        (stored.revision, stored.links.len()),
        (2, 0),
        "an empty list keeps the revision"
    );
}

#[test]
fn stored_remote_links_replay_to_the_same_state() {
    let mut w = World::new();
    let first = set(
        0,
        json!([link("requirement:api", "depends_on", true, Some(3))]),
    );
    w.ok("agent", first.clone());
    let saved = serde_json::to_value(&w.state.remote_links).unwrap();
    assert_eq!(saved["domain:d"]["links"][0]["target"], "requirement:api");
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
}
