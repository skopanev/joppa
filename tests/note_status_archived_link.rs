//! A Note keeps a related edge to an object that is archived later: its status can still
//! change.
mod common;
#[path = "common/relation_edges.rs"]
mod relation_edges;

use common::*;
use serde_json::json;

fn note(w: &mut World, id: &str, relations: serde_json::Value) {
    w.ok(
        "agent",
        json!({"op":"save_object","kind":"note","id":id,"revision":0,
        "data":{"title":id,"owner":"owner","body":"Source text","source":"test"},
        "relations":relations,"reason":"Record an input"}),
    );
}

#[test]
fn a_note_that_links_to_an_archived_object_changes_its_status() {
    let mut w = World::new();
    note(&mut w, "old", json!({}));
    note(
        &mut w,
        "input",
        relation_edges::one("related", &["note:old"]),
    );
    w.ok("owner", json!({"op":"archive_object","target":"note:old","revision":1,"reason":"Archive the old input"}));
    w.ok("owner", json!({"op":"decide_note","target":"note:input","revision":1,"outcome":"rejected","reason":"The input is processed"}));
    let r = w.state.objects["note:input"].current();
    assert_eq!(r.data["status"], "processed");
    let stored = relation_edges::of(r);
    assert_eq!(
        relation_edges::targets(&stored, "related"),
        ["note:old"],
        "the Note keeps its edge: {stored}"
    );
}

#[test]
fn a_new_link_to_an_archived_object_is_refused() {
    let mut w = World::new();
    note(&mut w, "old", json!({}));
    note(&mut w, "input", json!({}));
    w.ok("owner", json!({"op":"archive_object","target":"note:old","revision":1,"reason":"Archive the old input"}));
    let err = w
        .apply(
            "agent",
            json!({"op":"save_object","kind":"note","id":"input","revision":1,
        "data":{"title":"input","owner":"owner","body":"Source text","source":"test"},
        "relations":{"related":{"edges":[{"to":"note:old"}]}},"reason":"Link the old input"}),
        )
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("linked object is absent from the current tree"),
        "{err}"
    );
}
