//! requirement:edit-makes-revision: a live save without links gets the current links before
//! the journal stores it, and a journal event without links still gives the object none.
mod common;

use common::*;
use joppa::commands::{self, Command, SaveObjectOp};
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
    let link = json!([{"target":"ac:key","relation":"related"}]);
    w.ok(
        "agent",
        json!({"op":"save_object","kind":"ac","id":"ios","revision":current.revision,
        "parent":current.parent,"data":current.data,"links":link,"reason":"Link the key"}),
    );
    let current = w.state.objects["ac:ios"].current().clone();
    let mut data = current.data.clone();
    data["body"] = json!("Given a paid user on iOS, then access stays.");
    let save: Command = serde_json::from_value(json!({"op":"save_object","kind":"ac","id":"ios",
        "revision":current.revision,"parent":current.parent,"data":data,"reason":"Clarify"}))
    .unwrap();
    let mut live = save.clone();
    joppa::omitted_links::keep(&w.state, &mut live);
    let Command::SaveObject(SaveObjectOp {
        links: Some(links), ..
    }) = &live
    else {
        panic!("the live save names its links: {live:?}");
    };
    assert_eq!(
        links, &current.links,
        "the live save keeps the current links"
    );
    commands::apply_replay(&mut w.state, "agent", &save, w.position + 1, "now").unwrap();
    assert!(
        w.state.objects["ac:ios"].current().links.is_empty(),
        "a journal event without links gives none"
    );
}
