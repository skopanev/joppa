//! A save of one criterion changes only that criterion and, past own_requirement_revisions, adds
//! no revision to its specification.
mod common;

use common::*;
use joppa::commands::{self, Command};
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
               "parent":current.parent,"data":data,"reason":"Clarify the criterion"}),
    )
}

fn replay_edit(w: &mut World, key: &str, description: &str) {
    let id = key.split_once(':').unwrap().1;
    let current = w.state.objects[key].current().clone();
    let mut data = current.data.clone();
    data["body"] = json!(description);
    let cmd: Command = serde_json::from_value(json!({
        "op":"save_object","kind":"ac","id":id,"revision":current.revision,
        "parent":current.parent,"data":data,"reason":"Clarify the criterion"
    }))
    .unwrap();
    commands::apply_replay(&mut w.state, "agent", &cmd, w.position + 1, "now").unwrap();
    w.position += 1;
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
    w.state
        .replay_epochs
        .insert(commands::SINGLE_CRITERION_SAVE.into(), w.position + 100);
    w.state
        .replay_epochs
        .insert(commands::OWN_REQUIREMENT_REVISIONS.into(), w.position + 100);
    let sibling = revision(&w, "ac:key");
    replay_edit(
        &mut w,
        "ac:ios",
        "Given a paid user on iOS, then access stays.",
    );
    assert_eq!(revision(&w, "ac:key"), sibling + 1);
}

fn spec_reason(w: &World, criterion: &str) -> String {
    let spec = w.state.objects[criterion].current().parent.clone().unwrap();
    w.state.objects[&spec].current().reason.clone()
}

// ac:64asdbwi
#[test]
fn a_criterion_save_names_the_criterion_in_the_specification_reason() {
    // Between criterion_save_reason and own_requirement_revisions: R gets a new revision naming ac:ios.
    let mut w = migrated();
    let spec = w.state.objects["ac:ios"].current().parent.clone().unwrap();
    let before = revision(&w, &spec);
    w.state
        .replay_epochs
        .insert(commands::OWN_REQUIREMENT_REVISIONS.into(), w.position + 100);
    replay_edit(
        &mut w,
        "ac:ios",
        "Given a paid user on iOS, then access stays.",
    );
    assert_eq!(revision(&w, &spec), before + 1);
    assert_eq!(
        spec_reason(&w, "ac:ios"),
        "Criterion ac:ios changed: Clarify the criterion"
    );
    assert_eq!(
        w.state.objects["ac:ios"].current().reason,
        "Clarify the criterion"
    );

    // Live save past own_requirement_revisions: R gets no new revision, and ac:ios keeps its reason.
    let mut live = migrated();
    let live_before = revision(&live, &spec);
    edit(
        &mut live,
        "ac:ios",
        "Given a paid user on iOS, then access stays.",
    )
    .unwrap();
    assert_eq!(revision(&live, &spec), live_before);
    assert_eq!(
        live.state.objects["ac:ios"].current().reason,
        "Clarify the criterion"
    );
}

// ac:64asdbwi
#[test]
fn an_older_criterion_save_in_a_replay_keeps_the_copied_specification_reason() {
    let mut w = migrated();
    w.state
        .replay_epochs
        .insert(commands::CRITERION_SAVE_REASON.into(), w.position + 100);
    w.state
        .replay_epochs
        .insert(commands::OWN_REQUIREMENT_REVISIONS.into(), w.position + 100);
    replay_edit(
        &mut w,
        "ac:ios",
        "Given a paid user on iOS, then access stays.",
    );
    assert_eq!(spec_reason(&w, "ac:ios"), "Clarify the criterion");
}

// ac:29ximlsr
#[test]
fn a_criterion_save_keeps_the_contract_revision_of_its_specification() {
    // Live save: R gets no revision, and its contract revision stays current.
    let mut live = migrated();
    let spec = live.state.objects["ac:ios"]
        .current()
        .parent
        .clone()
        .unwrap();
    let (live_before, live_contract) = (
        revision(&live, &spec),
        joppa::objects::contract_revision(&live.state.objects[&spec]),
    );
    edit(
        &mut live,
        "ac:ios",
        "Given a paid user on iOS, then access stays.",
    )
    .unwrap();
    assert_eq!(revision(&live, &spec), live_before);
    assert_eq!(
        joppa::objects::contract_revision(&live.state.objects[&spec]),
        live_contract
    );

    // Replay between forced_keeps_contract and own_requirement_revisions: R gets a new revision with the same contract.
    let mut w = migrated();
    let (before, contract) = (
        revision(&w, &spec),
        joppa::objects::contract_revision(&w.state.objects[&spec]),
    );
    w.state
        .replay_epochs
        .insert(commands::OWN_REQUIREMENT_REVISIONS.into(), w.position + 100);
    replay_edit(
        &mut w,
        "ac:ios",
        "Given a paid user on iOS, then access stays.",
    );
    assert_eq!(
        revision(&w, &spec),
        before + 1,
        "the pre-epoch specification gets a revision"
    );
    assert_eq!(
        joppa::objects::contract_revision(&w.state.objects[&spec]),
        contract,
        "a pin of the specification stays current"
    );
}

// ac:29ximlsr
#[test]
fn an_older_criterion_save_in_a_replay_starts_a_new_contract_of_its_specification() {
    let mut w = migrated();
    w.state
        .replay_epochs
        .insert(commands::FORCED_KEEPS_CONTRACT.into(), w.position + 100);
    w.state
        .replay_epochs
        .insert(commands::OWN_REQUIREMENT_REVISIONS.into(), w.position + 100);
    let spec = w.state.objects["ac:ios"].current().parent.clone().unwrap();
    let before = revision(&w, &spec);
    replay_edit(
        &mut w,
        "ac:ios",
        "Given a paid user on iOS, then access stays.",
    );
    assert_eq!(revision(&w, &spec), before + 1);
    assert_eq!(
        joppa::objects::contract_revision(&w.state.objects[&spec]),
        before + 1,
        "the older save starts a new contract revision"
    );
}
