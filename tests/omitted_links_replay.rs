//! requirement:edit-makes-revision: a live save without relations keeps the current relations,
//! and a journal event without relations or links still gives the object none.
mod common;
#[path = "common/relation_edges.rs"]
mod relation_edges;

use common::*;
use joppa::commands::{self, Command};
use serde_json::json;

// edit-makes-revision-ac4
#[test]
fn an_event_without_links_replays_with_none_and_a_live_save_names_the_current_links() {
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
    let current = w.state.objects["ac:ios"].current().clone();
    w.ok(
        "agent",
        json!({"op":"save_object","kind":"ac","id":"ios","revision":current.revision,
        "parent":current.parent,"data":current.data,
        "relations":relation_edges::one("related", &["ac:key"]),"reason":"Link the key"}),
    );
    let current = w.state.objects["ac:ios"].current().clone();
    let mut data = current.data.clone();
    data["body"] = json!("Given a paid user on iOS, then access stays.");
    let save = json!({"op":"save_object","kind":"ac","id":"ios",
        "revision":current.revision,"parent":current.parent,"data":data,"reason":"Clarify"});
    // A live save without relations keeps the current relations.
    let mut live = World {
        state: w.state.clone(),
        position: w.position,
    };
    live.ok("agent", save.clone());
    let kept = relation_edges::of(live.state.objects["ac:ios"].current());
    assert_eq!(
        relation_edges::targets(&kept, "related"),
        ["ac:key"],
        "the live save keeps the current relations: {kept}"
    );
    // The journal event of a live save names the full relations after the merge.
    let save: Command = serde_json::from_value(save).unwrap();
    let mut event = save.clone();
    joppa::omitted_links::keep(&w.state, &mut event);
    let event = serde_json::to_value(&event).unwrap();
    assert_eq!(
        relation_edges::pairs(&event),
        [("related".to_owned(), "ac:key".to_owned())],
        "the live save names the current relations: {event}"
    );
    // A journal event without relations or links replays with none.
    commands::apply_replay(&mut w.state, "agent", &save, w.position + 1, "now").unwrap();
    let replayed = relation_edges::of(w.state.objects["ac:ios"].current());
    assert!(
        relation_edges::pairs(&replayed).is_empty(),
        "a journal event without relations gives none: {replayed}"
    );
}
