//! The removed guards: an agent may hold several Tasks, a completed Task takes a new revision,
//! any member may change a Task, and a criterion may move to another requirement.
mod common;
use common::*;
use joppa::objects;
use serde_json::{Value, json};

fn internal(w: &mut World, op: &str) {
    let command: joppa::commands::Command = serde_json::from_value(json!({ "op": op })).unwrap();
    joppa::commands::apply_replay(&mut w.state, "owner", &command, w.position + 1, "now")
        .unwrap_or_else(|e| panic!("{op}: {e}"));
    w.position += 1;
}

/// A current workspace: every migration ran, and both criteria are ready.
fn current() -> World {
    let mut w = World::new();
    w.ready();
    for op in [
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
        "migrate_consumers",
        "migrate_consumer_drops",
        "migrate_test_rules",
        "migrate_note_parents",
        "migrate_body",
        "migrate_purpose",
    ] {
        internal(&mut w, op);
    }
    w
}

fn revision(w: &World, key: &str) -> u32 {
    w.state.objects.get(key).map_or(0, |o| o.current().revision)
}

fn save(w: &mut World, actor: &str, key: &str, parent: &str, data: Value) -> anyhow::Result<()> {
    let (kind, id) = key.split_once(':').unwrap();
    w.apply(
        actor,
        json!({"op":"save_object","kind":kind,"id":id,"revision":revision(w, key),
        "parent":parent,"data":data,"reason":"Change the work"}),
    )
}

fn task(title: &str) -> Value {
    json!({"title":title,"owner":"owner","body":"Implement the agreed part"})
}

fn claim(w: &mut World, key: &str) -> anyhow::Result<()> {
    let revision = revision(w, key);
    w.apply(
        "agent",
        json!({"op":"claim_task","id":key,"revision":revision}),
    )
}

#[test]
fn an_agent_claims_a_second_task_while_it_holds_one() {
    let mut w = current();
    save(&mut w, "owner", "task:a", "ac:ios", task("A")).unwrap();
    save(&mut w, "owner", "task:b", "ac:key", task("B")).unwrap();
    claim(&mut w, "task:a").unwrap();
    claim(&mut w, "task:b").unwrap();
    for key in ["task:a", "task:b"] {
        assert_eq!(w.state.task_process[key].assignee.as_deref(), Some("agent"));
    }
}

#[test]
fn a_completed_task_takes_a_new_revision_from_any_member() {
    let mut w = current();
    save(&mut w, "owner", "task:a", "ac:ios", task("A")).unwrap();
    claim(&mut w, "task:a").unwrap();
    let revision = revision(&w, "task:a");
    w.ok(
        "agent",
        json!({"op":"complete_task","id":"task:a","revision":revision,
        "evidence":"A is done","commit":"a".repeat(40)}),
    );
    // A member who is not the owner, the holder or the creator changes the completed Task.
    save(
        &mut w,
        "legal",
        "task:a",
        "ac:ios",
        task("A, with the agreed name"),
    )
    .unwrap();
    let a = w.state.objects["task:a"].current();
    assert_eq!(a.data["title"], "A, with the agreed name");
    assert_eq!(a.actor, "legal");
    assert!(w.state.task_process["task:a"].done.is_some());
}

#[test]
fn a_criterion_moves_to_another_requirement_and_leaves_the_old_one() {
    let mut w = current();
    let other = json!({"title":"Keys stay safe","owner":"owner","body":"Only the key rule"});
    save(&mut w, "owner", "requirement:s", "capability:c", other).unwrap();
    let ios = revision(&w, "ac:ios");
    let mut key = w.state.objects["ac:key"].current().data.clone();
    key["title"] = json!("Before production key has exactly read and verify permissions");
    save(&mut w, "owner", "ac:key", "requirement:s", key).unwrap();
    let moved = w.state.objects["ac:key"].current();
    assert_eq!(moved.parent.as_deref(), Some("requirement:s"));
    let (req, _, _) = objects::criterion_context(&w.state, "ac:key").unwrap();
    assert_eq!(req, "requirement:s");
    let old = w.state.requirements["r"].revisions.last().unwrap();
    assert!(old.acs.iter().all(|ac| ac.id != "key"), "{:?}", old.acs);
    assert!(old.acs.iter().any(|ac| ac.id == "ios"));
    assert_eq!(
        w.state.objects["ac:ios"].current().parent.as_deref(),
        Some("requirement:r")
    );
    assert_eq!(revision(&w, "ac:ios"), ios);
    // A later write leaves both criteria and both requirements as they are.
    let before: Vec<u32> = ["ac:key", "ac:ios", "requirement:r", "requirement:s"]
        .iter()
        .map(|k| revision(&w, k))
        .collect();
    save(&mut w, "owner", "task:t", "ac:key", task("T")).unwrap();
    let after: Vec<u32> = ["ac:key", "ac:ios", "requirement:r", "requirement:s"]
        .iter()
        .map(|k| revision(&w, k))
        .collect();
    assert_eq!(before, after);
}
