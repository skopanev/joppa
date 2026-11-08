//! An archived criterion of a removed type does not block a save of its requirement.
mod common;

use common::*;
use joppa::{
    commands::{self, Command},
    objects,
};
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

/// A save of the object at `key` with its current fields and a new parent.
fn save(w: &mut World, key: &str, parent: Option<&str>) -> anyhow::Result<()> {
    let (kind, id) = key.split_once(':').unwrap();
    let current = w.state.objects[key].current().clone();
    let parent = parent.map(str::to_owned).or(current.parent.clone());
    w.apply(
        "owner",
        json!({"op":"save_object","kind":kind,"id":id,"revision":current.revision,
               "parent":parent,"data":current.data,"links":current.links,"reason":"Save again"}),
    )
}

fn schema_revision(w: &World) -> u32 {
    objects::schema(&w.state).revision
}

// ac:d6o49chu
#[test]
fn a_save_after_the_type_of_an_archived_criterion_is_removed_succeeds() {
    let mut w = migrated();
    let spec = w.state.objects["ac:ios"].current().parent.clone().unwrap();
    let mut tac: Value = serde_json::to_value(&objects::schema(&w.state).types["ac"]).unwrap();
    tac["title"] = json!("Technical criterion");
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":schema_revision(&w),"put_types":{"tac":tac},
               "reason":"Add a technical criterion"}),
    );
    w.ok(
        "owner",
        json!({"op":"save_object","kind":"tac","id":"t1","revision":0,"parent":spec,
               "data":{"title":"The key has a rotation date"},"reason":"Add a criterion"}),
    );
    w.ok(
        "owner",
        json!({"op":"archive_object","target":"tac:t1","revision":1,"reason":"Out of scope"}),
    );
    w.ok(
        "owner",
        json!({"op":"set_tree_schema","revision":schema_revision(&w),"remove_types":["tac"],
               "reason":"Remove the technical criterion"}),
    );
    w.ok(
        "owner",
        json!({"op":"save_object","kind":"capability","id":"c2","revision":0,"parent":"domain:d",
               "data":{"title":"Restore paid access","owner":"owner"},"reason":"Add a capability"}),
    );
    save(&mut w, &spec, Some("capability:c2")).expect("the requirement moves");
    assert_eq!(
        w.state.objects[&spec].current().parent.as_deref(),
        Some("capability:c2")
    );
    let kept = w.state.requirements.values().any(|q| {
        let r = q.revisions.last().unwrap();
        r.acs.iter().any(|a| a.id == "tac:t1")
    });
    assert!(
        kept,
        "the archived criterion stays in the revision as history"
    );
    save(&mut w, "ac:ios", None).expect("a live sibling criterion saves");
}
