//! Operations that no live write accepts. Old journals hold them, so `apply_replay` still
//! reads them; HTTP, MCP and the CLI refuse them before any state change.
use crate::commands::Command;

/// The operation names that live writes refuse.
pub const NAMES: [&str; 13] = [
    "domain",
    "edit_domain",
    "capability",
    "edit_capability",
    "consumer",
    "edit_consumer",
    "revise",
    "task",
    "amend_task",
    "describe",
    "set_domain_archived",
    "confirm_tree",
    "link_source",
];

/// The refusal of a removed operation: it names the replacement.
pub fn refusal(name: &str) -> String {
    let replacement = match name {
        "set_domain_archived" => "archive_object or restore_object",
        "confirm_tree" => "save_object; a saved tree needs no confirmation",
        "link_source" => "save_object with a Note and its links",
        _ => "save_object",
    };
    format!("{name} was removed; use {replacement}")
}

/// The name of a removed operation, or None for a live one.
pub fn name(command: &Command) -> Option<&'static str> {
    Some(match command {
        Command::Domain { .. } => "domain",
        Command::EditDomain { .. } => "edit_domain",
        Command::Capability { .. } => "capability",
        Command::EditCapability { .. } => "edit_capability",
        Command::Consumer { .. } => "consumer",
        Command::EditConsumer { .. } => "edit_consumer",
        Command::Revise { .. } => "revise",
        Command::Task { .. } => "task",
        Command::AmendTask { .. } => "amend_task",
        Command::Describe { .. } => "describe",
        Command::SetDomainArchived { .. } => "set_domain_archived",
        Command::ConfirmTree {} => "confirm_tree",
        Command::LinkSource { .. } => "link_source",
        _ => return None,
    })
}
