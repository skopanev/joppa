//! Operations and fields that no live write accepts. Old journals hold them, so `apply_replay` still
//! reads them; HTTP, MCP and the CLI refuse them before any state change.
use crate::commands::Command;

/// The operation names that live writes refuse.
pub const NAMES: [&str; 25] = [
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
    "plan",
    "result",
    "finish",
    "accept",
    "revoke",
    "issue",
    "resolve",
    "review_coverage",
    "define_verification_scope",
    "set_note_status",
    "confirm",
    "decide_note",
];

/// The refusal of a removed operation: it names the replacement.
pub fn refusal(name: &str) -> String {
    let replacement = match name {
        "set_domain_archived" => "archive_object or restore_object",
        "confirm_tree" => "save_object; a saved tree needs no confirmation",
        "link_source" => "save_object with a Note and its links",
        "plan" | "result" | "finish" => "joppa_record_results",
        "accept" | "revoke" => "accept_commit",
        "issue" | "resolve" => "note",
        "review_coverage" | "define_verification_scope" => "set_ac_tests",
        "set_note_status" | "decide_note" => "archive_object",
        "confirm" => "save_object; a saved requirement needs no confirmation",
        _ => "save_object",
    };
    format!("{name} was removed; use {replacement}")
}

/// The name of a removed operation, or None for a live one.
pub fn name(command: &Command) -> Option<&'static str> {
    Some(match command {
        Command::Domain(crate::commands::DomainOp { .. }) => "domain",
        Command::EditDomain(crate::commands::EditDomainOp { .. }) => "edit_domain",
        Command::Capability(crate::commands::CapabilityOp { .. }) => "capability",
        Command::EditCapability(crate::commands::EditCapabilityOp { .. }) => "edit_capability",
        Command::Consumer(crate::commands::ConsumerOp { .. }) => "consumer",
        Command::EditConsumer(crate::commands::EditConsumerOp { .. }) => "edit_consumer",
        Command::Revise(crate::commands::ReviseOp { .. }) => "revise",
        Command::Task(crate::commands::TaskOp { .. }) => "task",
        Command::AmendTask(crate::commands::AmendTaskOp { .. }) => "amend_task",
        Command::Describe(crate::commands::DescribeOp { .. }) => "describe",
        Command::SetDomainArchived(crate::commands::SetDomainArchivedOp { .. }) => {
            "set_domain_archived"
        }
        Command::ConfirmTree(crate::commands::ConfirmTreeOp {}) => "confirm_tree",
        Command::LinkSource(crate::commands::LinkSourceOp { .. }) => "link_source",
        Command::Plan(crate::commands::PlanOp { .. }) => "plan",
        Command::Result(crate::commands::ResultOp { .. }) => "result",
        Command::Finish(crate::commands::FinishOp { .. }) => "finish",
        Command::Accept(crate::commands::AcceptOp { .. }) => "accept",
        Command::Revoke(crate::commands::RevokeOp { .. }) => "revoke",
        Command::Issue(crate::commands::IssueOp { .. }) => "issue",
        Command::Resolve(crate::commands::ResolveOp { .. }) => "resolve",
        Command::ReviewCoverage(crate::commands::ReviewCoverageOp { .. }) => "review_coverage",
        Command::DefineVerificationScope(crate::commands::DefineVerificationScopeOp { .. }) => {
            "define_verification_scope"
        }
        Command::SetNoteStatus(crate::commands::SetNoteStatusOp { .. }) => "set_note_status",
        Command::Confirm { .. } => "confirm",
        Command::DecideNote(crate::commands::DecideNoteOp { .. }) => "decide_note",
        _ => return None,
    })
}

/// The refusal of a removed field of a live operation, or None when the command uses none.
pub fn removed_field(command: &Command) -> Option<&'static str> {
    match command {
        Command::Comment(crate::commands::CommentOp { kind, .. }) if kind == "reject" => {
            Some("comment kind reject was removed; use kind comment")
        }
        Command::SetTreeSchema(crate::commands::SetTreeSchemaOp {
            status_colors: Some(_),
            ..
        }) => Some("status_colors was removed; use statuses"),
        _ => None,
    }
}
