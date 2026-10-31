//! A save of one criterion changes only that criterion, and an unassessed criterion
//! names its next step.
mod common;

use common::*;
use joppa::{
    commands::{self, Command},
    objects,
};
use serde_json::json;

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

fn revision(w: &World, key: &str) -> u32 {
    w.state.objects[key].current().revision
}

fn edit(w: &mut World, key: &str, description: &str) -> anyhow::Result<()> {
    let id = key.split_once(':').unwrap().1;
    let current = w.state.objects[key].current().clone();
    let mut data = current.data.clone();
    data["body"] = json!(description);
    w.apply(
        "agent",
        json!({"op":"save_object","kind":"ac","id":id,"revision":current.revision,
               "parent":current.parent,"data":data,"links":current.links,"reason":"Clarify the criterion"}),
    )
}

#[test]
fn a_criterion_save_keeps_the_revision_of_its_sibling() {
    let mut w = migrated();
    let sibling = revision(&w, "ac:key");
    let saved = revision(&w, "ac:ios");
    edit(
        &mut w,
        "ac:ios",
        "Given a paid user on iOS, when the user logs in again, then access stays.",
    )
    .unwrap();
    assert_eq!(revision(&w, "ac:ios"), saved + 1);
    assert_eq!(
        revision(&w, "ac:key"),
        sibling,
        "the sibling keeps its revision"
    );
    edit(
        &mut w,
        "ac:key",
        "Given the production key, then it has only read and verify.",
    )
    .expect("the sibling saves with the revision that was read before");
}

#[test]
fn an_older_criterion_save_in_a_replay_keeps_the_sibling_revision_it_gave() {
    let mut w = migrated();
    // The journal reached this rule after the save, so the replay keeps the old effect.
    w.state
        .replay_epochs
        .insert(commands::SINGLE_CRITERION_SAVE.into(), w.position + 100);
    let current = w.state.objects["ac:ios"].current().clone();
    let mut data = current.data.clone();
    data["body"] = json!("Given a paid user on iOS, then access stays.");
    let cmd: Command = serde_json::from_value(json!({"op":"save_object","kind":"ac","id":"ios",
        "revision":current.revision,"parent":current.parent,"data":data,"links":current.links,
        "reason":"Clarify the criterion"}))
    .unwrap();
    let sibling = revision(&w, "ac:key");
    commands::apply_replay(&mut w.state, "agent", &cmd, w.position + 1, "now").unwrap();
    assert_eq!(revision(&w, "ac:key"), sibling + 1);
}

fn spec_reason(w: &World, criterion: &str) -> String {
    let spec = w.state.objects[criterion].current().parent.clone().unwrap();
    w.state.objects[&spec].current().reason.clone()
}

#[test]
fn a_criterion_save_names_the_criterion_in_the_specification_reason() {
    let mut w = migrated();
    edit(
        &mut w,
        "ac:ios",
        "Given a paid user on iOS, then access stays.",
    )
    .unwrap();
    assert_eq!(
        spec_reason(&w, "ac:ios"),
        "Criterion ac:ios changed: Clarify the criterion"
    );
    assert_eq!(
        w.state.objects["ac:ios"].current().reason,
        "Clarify the criterion"
    );
}

#[test]
fn an_older_criterion_save_in_a_replay_keeps_the_copied_specification_reason() {
    let mut w = migrated();
    w.state
        .replay_epochs
        .insert(commands::CRITERION_SAVE_REASON.into(), w.position + 100);
    let current = w.state.objects["ac:ios"].current().clone();
    let mut data = current.data.clone();
    data["body"] = json!("Given a paid user on iOS, then access stays.");
    let cmd: Command = serde_json::from_value(json!({"op":"save_object","kind":"ac","id":"ios",
        "revision":current.revision,"parent":current.parent,"data":data,"links":current.links,
        "reason":"Clarify the criterion"}))
    .unwrap();
    commands::apply_replay(&mut w.state, "agent", &cmd, w.position + 1, "now").unwrap();
    assert_eq!(spec_reason(&w, "ac:ios"), "Clarify the criterion");
}

#[test]
fn an_unassessed_criterion_names_the_assess_command_as_its_next_step() {
    let w = migrated();
    let assessment = objects::assessment(&w.state, "ac:ios").unwrap();
    assert_eq!(assessment["state"], "unassessed");
    assert_eq!(
        assessment["next"],
        json!({"op":"assess","req":assessment["req"],"revision":assessment["revision"],"ac":"ios"})
    );
}
