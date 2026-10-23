//! set_tree_schema changes single type nodes with put_types and remove_types.
mod common;

use common::World;
use joppa::{
    commands::{self, Command},
    model::Workspace,
    objects,
};
use serde_json::{Value, json};

fn migrated() -> World {
    let mut w = World {
        state: Workspace::default(),
        position: 0,
    };
    w.ok("owner", json!({"op":"workspace","title":"Schema nodes"}));
    w.ok(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    );
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

fn types(w: &World) -> Value {
    serde_json::to_value(objects::schema(&w.state).types).unwrap()
}

fn revision(w: &World) -> u32 {
    objects::schema(&w.state).revision
}

fn epic() -> Value {
    json!({"title":"Epic","role":"group","root":true,"parents":[]})
}

#[test]
fn put_types_adds_one_type_and_keeps_every_other_type() {
    let mut w = migrated();
    let before = types(&w);
    let r = revision(&w);
    w.ok(
        "agent",
        json!({"op":"set_tree_schema","revision":r,"put_types":{"epic":epic()},"reason":"Add an epic level"}),
    );
    let after = types(&w);
    assert_eq!(revision(&w), r + 1);
    assert_eq!(after["epic"]["title"], "Epic");
    for (kind, node) in before.as_object().unwrap() {
        assert_eq!(&after[kind], node, "{kind} must stay unchanged");
    }
}

#[test]
fn put_types_replaces_one_type_completely() {
    let mut w = migrated();
    let r = revision(&w);
    let mut note = types(&w)["note"].clone();
    note["title"] = json!("Input");
    note["badge"] = json!("IN");
    w.ok(
        "agent",
        json!({"op":"set_tree_schema","revision":r,"put_types":{"note":note},"reason":"Rename the note type"}),
    );
    assert_eq!(types(&w)["note"], note);
}

#[test]
fn remove_types_deletes_an_unused_type() {
    let mut w = migrated();
    let r = revision(&w);
    w.ok(
        "agent",
        json!({"op":"set_tree_schema","revision":r,"put_types":{"epic":epic()},"reason":"Add an epic level"}),
    );
    w.ok(
        "agent",
        json!({"op":"set_tree_schema","revision":r+1,"remove_types":["epic"],"reason":"Remove the epic level"}),
    );
    assert!(types(&w).get("epic").is_none());
    assert_eq!(revision(&w), r + 2);
}

#[test]
fn remove_types_refuses_a_used_type_and_an_unknown_type() {
    let mut w = migrated();
    let r = revision(&w);
    w.ok(
        "agent",
        json!({"op":"save_object","kind":"note","id":"n1","revision":0,"parent":null,
               "data":{"title":"An input","body":"Text","owner":"owner"},"reason":"Store an input"}),
    );
    for (remove, error) in [
        (
            json!(["note"]),
            "object type is absent from the tree schema",
        ),
        (
            json!(["missing"]),
            "remove_types names an unknown type: missing",
        ),
        (json!(["ac"]), "unknown parent type"),
    ] {
        let e = w
            .apply(
                "agent",
                json!({"op":"set_tree_schema","revision":r,"remove_types":remove,"reason":"Remove"}),
            )
            .expect_err("the removal must be refused");
        assert!(e.to_string().contains(error), "{remove}: {e}");
    }
    assert_eq!(revision(&w), r, "a refused change creates no revision");
    assert!(types(&w).get("note").is_some());
}

#[test]
fn types_cannot_join_put_types_or_remove_types() {
    let mut w = migrated();
    let r = revision(&w);
    let all = types(&w);
    for extra in [
        json!({"put_types":{"epic":epic()}}),
        json!({"remove_types":["note"]}),
    ] {
        let mut command = json!({"op":"set_tree_schema","revision":r,"types":all,"reason":"Mixed"});
        command
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let e = w
            .apply("agent", command)
            .expect_err("a mixed command must be refused");
        assert!(
            e.to_string().contains("send types for the whole map"),
            "{e}"
        );
    }
    let e = w
        .apply(
            "agent",
            json!({"op":"set_tree_schema","revision":r,"put_types":{"epic":epic()},"remove_types":["epic"],"reason":"Both"}),
        )
        .expect_err("one type in both lists must be refused");
    assert!(
        e.to_string().contains("in put_types and in remove_types"),
        "{e}"
    );
}

#[test]
fn a_command_without_types_keeps_the_map_and_changes_priorities() {
    let mut w = migrated();
    let before = types(&w);
    let r = revision(&w);
    w.ok(
        "agent",
        json!({"op":"set_tree_schema","revision":r,"priorities":["now","later"],"reason":"Two priorities"}),
    );
    assert_eq!(types(&w), before);
    assert_eq!(
        objects::schema(&w.state).priorities,
        Some(vec!["now".to_owned(), "later".to_owned()])
    );
}

#[test]
fn a_stale_revision_is_refused_for_a_node_change() {
    let mut w = migrated();
    let r = revision(&w);
    let e = w
        .apply(
            "agent",
            json!({"op":"set_tree_schema","revision":r+5,"put_types":{"epic":epic()},"reason":"Stale"}),
        )
        .expect_err("a stale revision must be refused");
    assert!(e.to_string().contains("stale schema revision"), "{e}");
}

#[test]
fn a_replayed_node_change_rebuilds_the_same_schema() {
    let mut w = migrated();
    let mut journal: Vec<Command> = vec![];
    let r = revision(&w);
    for value in [
        json!({"op":"set_tree_schema","revision":r,"put_types":{"epic":epic()},"reason":"Add"}),
        json!({"op":"set_tree_schema","revision":r+1,"remove_types":["epic"],"put_types":{"release":epic()},"reason":"Swap"}),
    ] {
        let cmd: Command = serde_json::from_value(value.clone()).unwrap();
        // The journal stores the serialized command, so replay reads that form.
        journal.push(serde_json::from_value(serde_json::to_value(&cmd).unwrap()).unwrap());
        w.ok("agent", value);
    }
    let mut replay = migrated();
    for cmd in &journal {
        commands::apply_replay(&mut replay.state, "agent", cmd, replay.position + 1, "now")
            .unwrap();
        replay.position += 1;
    }
    assert_eq!(types(&replay), types(&w));
    assert!(types(&w).get("release").is_some() && types(&w).get("epic").is_none());
}

#[test]
fn one_command_removes_a_reference_target_and_its_user_together() {
    let mut w = migrated();
    let r = revision(&w);
    let release = json!({"title":"Release","role":"group","root":true,"parents":[],
                         "references":{"epic":{"target":"epic","title":"Epic"}}});
    w.ok(
        "agent",
        json!({"op":"set_tree_schema","revision":r,"put_types":{"epic":epic(),"release":release},"reason":"Add two types"}),
    );
    let e = w
        .apply(
            "agent",
            json!({"op":"set_tree_schema","revision":r+1,"remove_types":["epic"],"reason":"Remove the target only"}),
        )
        .expect_err("a reference to a removed type must be refused");
    assert!(
        e.to_string().contains("unknown reference target type"),
        "{e}"
    );
    w.ok(
        "agent",
        json!({"op":"set_tree_schema","revision":r+1,"remove_types":["epic","release"],"reason":"Remove both"}),
    );
    assert!(types(&w).get("epic").is_none() && types(&w).get("release").is_none());
}
