//! A replay writes each save_object into the workspace without a copy of it, and it gives the
//! same workspace as the live writes of the same events. It skips the document sync of an
//! event that changes no document.
mod common;

use common::*;
use joppa::commands::{self, Command};
use joppa::probe;
use serde_json::{Value, json};

fn migrated() -> World {
    let mut w = World::new();
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
    ] {
        let cmd: Command = serde_json::from_value(json!({ "op": op })).unwrap();
        commands::apply_replay(&mut w.state, "owner", &cmd, w.position + 1, "now").unwrap();
        w.position += 1;
    }
    w
}

/// A save of a new body of a criterion of `w`.
fn edit(w: &World, key: &str, body: &str) -> Value {
    let current = w.state.objects[key].current();
    let mut data = current.data.clone();
    data["body"] = json!(body);
    json!({"op":"save_object","kind":"ac","id":key.split_once(':').unwrap().1,
        "revision":current.revision,"parent":current.parent,"data":data,
        "links":current.links,"reason":"Clarify the criterion"})
}

// ac:499ipqmz
#[test]
fn a_replay_saves_without_a_copy_and_gives_the_workspace_of_the_live_writes() {
    let (mut live, mut replay) = (migrated(), migrated());
    let mut events = vec![];
    for step in 0..20 {
        let key = if step % 2 == 0 { "ac:ios" } else { "ac:key" };
        let event = edit(
            &live,
            key,
            &format!("Given step {step}, then access stays."),
        );
        live.ok("agent", event.clone());
        events.push(event);
    }
    let copies = probe::read(&probe::SAVE_COPY);
    for event in events {
        let cmd: Command = serde_json::from_value(event).unwrap();
        commands::apply_replay(
            &mut replay.state,
            "agent",
            &cmd,
            replay.position + 1,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
        replay.position += 1;
    }
    assert_eq!(
        probe::read(&probe::SAVE_COPY),
        copies,
        "a replay copies no workspace"
    );
    assert_eq!(
        serde_json::to_value(&replay.state.objects).unwrap(),
        serde_json::to_value(&live.state.objects).unwrap(),
        "the replay gives the objects of the live writes"
    );
}

// ac:db4wfihl
#[test]
fn a_replay_skips_the_sync_only_of_events_that_change_no_document() {
    let mut w = migrated();
    let events = [
        (
            "agent",
            json!({"op":"note","target":"ac:ios","id":"n1","text":"Seen"}),
        ),
        ("owner", json!({"op":"member","id":"lena","role":"human"})),
        (
            "agent",
            json!({"op":"assess","target":"ac:key","ready":false,"reason":"Waits"}),
        ),
        ("owner", json!({"op":"update_workspace","hidden":true})),
        (
            "owner",
            json!({"op":"remove_member","id":"lena","reason":"Left the team"}),
        ),
    ];
    for (actor, event) in events {
        let cmd: Command = serde_json::from_value(event).unwrap();
        let at = "2026-01-01T00:00:00Z";
        commands::apply_replay(&mut w.state, actor, &cmd, w.position + 1, at).unwrap();
        w.position += 1;
        // The skipped sync of the event would change no object.
        let mut synced = w.state.clone();
        joppa::objects::sync(&mut synced, actor, &cmd, w.position, at);
        assert_eq!(
            serde_json::to_value(&synced.objects).unwrap(),
            serde_json::to_value(&w.state.objects).unwrap(),
            "{cmd:?}"
        );
    }
}
