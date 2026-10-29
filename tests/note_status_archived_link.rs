//! A Note keeps a link to an object that is archived later: its status can still change.
mod common;

use common::*;
use serde_json::json;

fn note(w: &mut World, id: &str, links: serde_json::Value) {
    w.ok(
        "agent",
        json!({"op":"save_object","kind":"note","id":id,"revision":0,
        "data":{"title":id,"owner":"owner","body":"Source text","source":"test"},
        "links":links,"reason":"Record an input"}),
    );
}

#[test]
fn a_note_that_links_to_an_archived_object_changes_its_status() {
    let mut w = World::new();
    note(&mut w, "old", json!([]));
    note(
        &mut w,
        "input",
        json!([{"target":"note:old","relation":"related","blocking":false}]),
    );
    w.ok("owner", json!({"op":"archive_object","target":"note:old","revision":1,"reason":"Archive the old input"}));
    w.ok("owner", json!({"op":"decide_note","target":"note:input","revision":1,"outcome":"rejected","reason":"The input is processed"}));
    let r = w.state.objects["note:input"].current();
    assert_eq!(r.data["status"], "processed");
    assert_eq!(r.links.len(), 1, "the Note keeps its link");
    assert_eq!(r.links[0].target, "note:old");
}

#[test]
fn a_new_link_to_an_archived_object_is_refused() {
    let mut w = World::new();
    note(&mut w, "old", json!([]));
    note(&mut w, "input", json!([]));
    w.ok("owner", json!({"op":"archive_object","target":"note:old","revision":1,"reason":"Archive the old input"}));
    let err = w.apply("agent", json!({"op":"save_object","kind":"note","id":"input","revision":1,
        "data":{"title":"input","owner":"owner","body":"Source text","source":"test"},
        "links":[{"target":"note:old","relation":"related","blocking":false}],"reason":"Link the old input"})).unwrap_err().to_string();
    assert!(
        err.contains("linked object is absent from the current tree"),
        "{err}"
    );
}
