//! Operations that no live write accepts. Old journals hold them, so `apply_replay` still
//! reads them; HTTP, MCP and the CLI refuse them before any state change.
use crate::commands::Command;

/// The operation names that live writes refuse.
pub const NAMES: [&str; 10] = [
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
];

/// The refusal of a removed operation: it names the replacement.
pub fn refusal(name: &str) -> String {
    format!("{name} was removed; use save_object")
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
        _ => return None,
    })
}
