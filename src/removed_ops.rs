//! Operations and fields that no live write accepts. Old journals hold them, so `apply_replay` still
//! reads them; HTTP, MCP and the CLI refuse them before any state change.
use crate::commands::Command;

/// The operation names that live writes refuse.
pub const NAMES: &[&str] = &[
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
    "set_ac_tests",
    "assess_signal",
    "set_priority",
    "claim_planning",
    "finish_planning",
    "release_planning",
    "claim_task",
    "claim_next_task",
    "heartbeat_task",
    "release_task",
    "block_task",
    "unblock_task",
    "complete_task",
    "review_task",
    "retire_task",
    "comment",
];

/// The refusal of the removed read parameter `work_filter`.
pub const WORK_FILTER: &str =
    "work_filter was removed; use the node filters, for example kind and ready";

/// The refusal of a removed operation: it names the replacement.
pub fn refusal(name: &str) -> String {
    let replacement = match name {
        "set_domain_archived" => "archive_object or restore_object",
        "confirm_tree" => "save_object; a saved tree needs no confirmation",
        "link_source" => "save_object with a Note and its links",
        "plan" | "result" | "finish" => "joppa_record_results",
        "accept" | "revoke" => "accept_commit",
        "issue" | "resolve" => "note",
        "review_coverage" | "define_verification_scope" | "set_ac_tests" => {
            "save_object with kind test"
        }
        "set_note_status" | "decide_note" => "archive_object",
        "confirm" => "save_object; a saved requirement needs no confirmation",
        "assess_signal" | "block_task" | "unblock_task" => "assess",
        "set_priority" => "save_object with data.priority",
        "claim_planning" | "finish_planning" | "release_planning" | "claim_task"
        | "claim_next_task" | "heartbeat_task" | "release_task" => "save_object with data.assignee",
        "complete_task" | "retire_task" => "archive_object",
        "comment" => "note",
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
        Command::SetAcTests(crate::commands::SetAcTestsOp { .. }) => "set_ac_tests",
        Command::AssessSignal { .. } => "assess_signal",
        Command::SetPriority { .. } => "set_priority",
        Command::ClaimPlanning { .. } => "claim_planning",
        Command::FinishPlanning { .. } => "finish_planning",
        Command::ReleasePlanning { .. } => "release_planning",
        Command::ClaimTask { .. } => "claim_task",
        Command::HeartbeatTask { .. } => "heartbeat_task",
        Command::ReleaseTask { .. } => "release_task",
        Command::BlockTask { .. } => "block_task",
        Command::UnblockTask { .. } => "unblock_task",
        Command::CompleteTask(crate::commands::CompleteTaskOp { .. }) => "complete_task",
        Command::ReviewTask(crate::commands::ReviewTaskOp { .. }) => "review_task",
        Command::RetireTask { .. } => "retire_task",
        Command::Comment(crate::commands::CommentOp { .. }) => "comment",
        _ => return None,
    })
}

/// The refusal of a removed field of a live operation, or None when the command uses none.
pub fn removed_field(command: &Command) -> Option<&'static str> {
    match command {
        Command::Assess(crate::commands::AssessOp::Criterion(_)) => {
            Some("assess with req, revision and ac was removed; use assess {target, ready, reason}")
        }
        Command::SetTreeSchema(crate::commands::SetTreeSchemaOp {
            status_colors: Some(_),
            ..
        }) => Some("status_colors was removed; use statuses"),
        // A test result field has no per-value board status: a test is a node with one tone.
        Command::SetTreeSchema(crate::commands::SetTreeSchemaOp {
            test_results: Some(block),
            ..
        }) if block["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|f| f.get("board").is_some_and(|b| b != false) || f.get("applies").is_some()) =>
        {
            Some("board and applies were removed from result fields; filter test nodes by status")
        }
        _ => None,
    }
}
