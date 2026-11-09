//! Part of `commands`: the replay rules and their epochs.
use super::*;

/// Replay rules: each one is a later change of the meaning of an old command. A rule
/// applies to every live write, and in a replay from its epoch in the workspace on.
pub const RESET_CHANGED: &str = "reset_changed";

pub const LIST_SAVE_RESET: &str = "list_save_reset";

/// From this rule on, the member command accepts only people. Agents and runners enroll
/// through the internal enroll_service event.
pub const HUMAN_MEMBERS: &str = "human_members";

/// From this rule on, a save of one criterion gives a new revision only to that criterion
/// and its specification. Unchanged sibling criteria keep their revisions.
pub const SINGLE_CRITERION_SAVE: &str = "single_criterion_save";

/// From this rule on, the specification revision that a criterion save gives names that
/// criterion and its reason. An older save keeps the copied reason in a replay.
pub const CRITERION_SAVE_REASON: &str = "criterion_save_reason";

/// From this rule on, an archive resets the criteria at and below the archived object. An
/// older archive keeps its reset at and below the parent in a replay.
pub const ARCHIVE_OWN_RESET: &str = "archive_own_reset";

/// From this rule on, any new parent, link or data value of a node resets the criteria at and
/// below it. An older revision resets them only for a new parent, link, text or test list in
/// a replay.
pub const ANY_CHANGE_RESET: &str = "any_change_reset";

/// From this rule on, a revision that only a child save forces keeps the contract revision of
/// the document. An older one starts a new contract revision in a replay.
pub const FORCED_KEEPS_CONTRACT: &str = "forced_keeps_contract";

/// From this rule on, a new title of a test node resets nothing, and the node keeps its earlier
/// title for the results that an older record stored by title. An older rename resets the
/// node in a replay.
pub const RENAME_KEEPS_RESULTS: &str = "rename_keeps_results";

/// From this rule on, a new title, body or file of a node of a source type resets each
/// criterion that names it or links with it. An older event resets none of them in a replay.
pub const SOURCE_CHANGE_RESET: &str = "source_change_reset";

/// From this rule on, only a change of the depends_on and related edges resets, and a flow
/// never does. An older event compares its links in their older form in a replay.
pub const RELATIONS_RESET: &str = "relations_reset";

pub const REPLAY_RULES: [&str; 11] = [
    RESET_CHANGED,
    LIST_SAVE_RESET,
    HUMAN_MEMBERS,
    SINGLE_CRITERION_SAVE,
    CRITERION_SAVE_REASON,
    ARCHIVE_OWN_RESET,
    ANY_CHANGE_RESET,
    FORCED_KEEPS_CONTRACT,
    RENAME_KEEPS_RESULTS,
    SOURCE_CHANGE_RESET,
    RELATIONS_RESET,
];

/// The workspace journal reached the epoch of the rule at `position`.
pub(crate) fn epoch_reached(w: &Workspace, rule: &str, position: u64) -> bool {
    w.replay_epochs
        .get(rule)
        .is_none_or(|first| position >= *first)
}

/// The rule applies to the event at `position`.
pub fn rule_applies(w: &Workspace, rule: &str, position: u64) -> bool {
    !w.replaying || epoch_reached(w, rule, position)
}
