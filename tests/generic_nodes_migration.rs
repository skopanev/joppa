//! migrate_generic_nodes moves the Task records, the priorities and the criterion
//! assessments of an old journal into the nodes once. Live writes then refuse the Task
//! operations.
mod common;
use common::*;
use joppa::objects;
use serde_json::{Value, json};

/// Apply an internal migration event, as the journal replay does.
fn internal(w: &mut World, op: &str) {
    let command: joppa::commands::Command = serde_json::from_value(json!({ "op": op })).unwrap();
    joppa::commands::apply_replay(
        &mut w.state,
        "owner",
        &command,
        w.position + 1,
        "2026-01-01T00:00:00Z",
    )
    .unwrap_or_else(|e| panic!("{op}: {e}"));
    w.position += 1;
}

fn data(w: &World, key: &str) -> Value {
    w.state.objects[key].current().data.clone()
}

/// The revision of a node, as a Task operation of the old journal names it.
fn revision(w: &World, key: &str) -> u32 {
    w.state.objects[key].current().revision
}

/// An old journal: four Tasks under a criterion, a claim, a completion, a retirement, a
/// priority and a criterion that waits for an answer of legal.
fn old_journal() -> World {
    let mut w = World::new();
    w.ready();
    w.ok("agent", assess("key", false));
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
    ] {
        internal(&mut w, op);
    }
    for id in ["held", "done", "retired", "open"] {
        let task =
            json!({"title":id,"owner":"owner","contract":"Implement the part","projects":[]});
        w.ok(
            "agent",
            json!({"op":"save_object","kind":"task","id":id,"revision":0,
            "parent":"ac:ios","data":task,"reason":"Plan the criterion"}),
        );
    }
    for (op, key, extra) in [
        ("claim_task", "task:held", json!({})),
        ("claim_task", "task:done", json!({})),
        (
            "complete_task",
            "task:done",
            json!({"evidence":"Merged","commit":"a".repeat(40)}),
        ),
        (
            "retire_task",
            "task:retired",
            json!({"reason":"Out of scope"}),
        ),
    ] {
        let mut command = json!({"op":op,"id":key,"revision":revision(&w, key)});
        for (field, value) in extra.as_object().unwrap() {
            command[field] = value.clone();
        }
        w.legacy("agent", command);
    }
    w.legacy(
        "agent",
        json!({"op":"set_priority","target":"task:open","priority":"p0"}),
    );
    w
}

// generic-nodes-ac5
#[test]
fn migrate_generic_nodes_moves_the_task_records_priorities_and_marks_into_the_nodes() {
    let mut w = old_journal();
    let boundaries = w.state.criterion_boundaries.clone();
    internal(&mut w, "migrate_generic_nodes");
    assert!(w.state.generic_nodes);
    assert_eq!(data(&w, "task:held")["assignee"], "agent");
    assert!(w.state.objects["task:held"].current().present);
    let done = w.state.objects["task:done"].current();
    assert_eq!(
        (done.present, done.reason.as_str(), &done.data["commit"]),
        (false, "Task done: Merged", &json!("a".repeat(40)))
    );
    let retired = w.state.objects["task:retired"].current();
    assert_eq!(
        (retired.present, retired.reason.as_str()),
        (false, "Task retired: Out of scope")
    );
    assert_eq!(data(&w, "task:open")["priority"], "p0");
    assert!(data(&w, "task:open").get("assignee").is_none());
    assert!(w.state.task_process.is_empty() && w.state.priorities.is_empty());
    assert_eq!(
        objects::assessment(&w.state, "ac:ios").unwrap()["state"],
        "ready"
    );
    let key = objects::assessment(&w.state, "ac:key").unwrap();
    assert_eq!(
        (key["state"].as_str(), key["reason"].as_str()),
        (
            Some("waiting"),
            Some("Which exact permissions? (asks legal)")
        )
    );
    // The event resets no criterion, and a second run changes nothing.
    assert_eq!(w.state.criterion_boundaries, boundaries);
    let once = serde_json::to_value(&w.state).unwrap();
    internal(&mut w, "migrate_generic_nodes");
    assert_eq!(serde_json::to_value(&w.state).unwrap(), once);
    // A live claim is refused, and a save of a Task keeps no Task record.
    let claim = json!({"op":"claim_task","id":"task:open","revision":revision(&w, "task:open")});
    let refusal = w.apply("agent", claim).unwrap_err().to_string();
    assert_eq!(
        refusal,
        "claim_task was removed; use save_object with data.assignee"
    );
    let mut open = data(&w, "task:open");
    open["assignee"] = json!("legal");
    w.ok(
        "agent",
        json!({"op":"save_object","kind":"task","id":"open",
        "revision":revision(&w, "task:open"),"parent":"ac:ios","data":open,"reason":"Assign"}),
    );
    assert!(w.state.task_process.is_empty());
    assert_eq!(data(&w, "task:open")["assignee"], "legal");
}
