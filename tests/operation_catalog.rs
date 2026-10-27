//! The public operation catalog matches the accepted commands and their required fields.
use joppa::{commands, store};
use serde_json::json;
use std::collections::BTreeSet;

#[path = "common/operation_samples.rs"]
mod operation_samples;

/// Command variants that public writes refuse: internal events, retired inputs and CLI-only creation.
const NOT_PUBLIC: &[&str] = &[
    "migrate_persistent_claims",
    "migrate_notes",
    "migrate_tasks",
    "migrate_schema",
    "migrate_documents",
    "migrate_task_claims",
    "migrate_consumers",
    "migrate_consumer_drops",
    "enroll_member",
    "enroll_service",
    "remove_workspace_references",
    "record_report",
    "heartbeat_task",
    "intake",
    "split",
    "questions",
    "sort",
    "answer",
    "route",
    "workspace",
];
/// Public operation names without their own Command variant.
const ALIASES: &[&str] = &["claim_next_task"];
const CALLERS: &[&str] = &["member", "agent", "holder", "workspace owner"];

/// Every operation name that the Command enum accepts, read from the serde error.
fn variants() -> BTreeSet<String> {
    let error = serde_json::from_value::<commands::Command>(json!({"op":"not_a_variant"}))
        .unwrap_err()
        .to_string();
    let (_, list) = error
        .split_once("expected one of ")
        .unwrap_or_else(|| panic!("{error}"));
    list.split(", ")
        .map(|name| name.trim_matches(|c| c != '_' && !char::is_alphanumeric(c)))
        .map(String::from)
        .collect()
}

fn catalog() -> BTreeSet<&'static str> {
    commands::OPERATIONS.iter().map(|o| o.name).collect()
}

#[test]
fn every_accepted_public_operation_has_one_catalog_entry() {
    let variants = variants();
    assert!(variants.contains("save_object") && variants.contains("workspace"));
    assert_eq!(
        catalog().len(),
        commands::OPERATIONS.len(),
        "duplicate entry"
    );
    let expected: BTreeSet<&str> = variants
        .iter()
        .map(String::as_str)
        .filter(|name| !NOT_PUBLIC.contains(name))
        .chain(ALIASES.iter().copied())
        .collect();
    assert_eq!(catalog(), expected);
    for name in NOT_PUBLIC {
        assert!(variants.contains(*name), "{name} is not a command");
        assert!(commands::operation(name).is_none(), "{name} is public");
    }
}

#[test]
fn every_catalog_entry_names_known_callers() {
    for o in commands::OPERATIONS {
        assert!(!o.callers.is_empty(), "{} has no callers", o.name);
        for caller in o.callers {
            assert!(CALLERS.contains(caller), "{}: {caller}", o.name);
        }
    }
    let block = commands::operation("block_task").unwrap();
    assert_eq!(block.callers, ["holder", "workspace owner"]);
}

#[test]
fn help_required_fields_match_field_validation() {
    for o in commands::OPERATIONS {
        let command = operation_samples::call(o.name, o.required);
        if let Err(error) = store::check_fields(&command) {
            panic!("{}: help-required fields fail validation: {error}", o.name);
        }
        for field in o.required {
            let mut missing = command.clone();
            missing.as_object_mut().unwrap().remove(*field);
            let error = store::check_fields(&missing)
                .err()
                .unwrap_or_else(|| panic!("{} accepts a call without {field}", o.name));
            assert!(
                error.to_string().contains("missing field"),
                "{} without {field}: {error}",
                o.name
            );
        }
    }
}
