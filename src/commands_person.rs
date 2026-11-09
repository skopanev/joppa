//! Part of `commands`: `person` and the items after it, moved from commands.rs unchanged.
use super::*;

/// Decisions on requirements come from people; an agent may only propose them.
pub fn person(w: &Workspace, actor: &str) -> Result<()> {
    ensure!(
        crate::inbox::actor_type(w, actor) == "person",
        "only a person records this decision"
    );
    Ok(())
}

pub fn human(w: &Workspace, actor: &str) -> Result<()> {
    ensure!(
        w.members.get(actor).is_some_and(|r| r == "human"),
        "a human member is required"
    );
    Ok(())
}

pub fn revision<'a>(
    w: &'a mut Workspace,
    req: &str,
    n: u32,
    current: bool,
) -> Result<&'a mut Revision> {
    let r = w
        .requirements
        .get_mut(req)
        .ok_or_else(|| anyhow::anyhow!("unknown REQ"))?;
    ensure!(
        n > 0 && (!current || r.revisions.last().is_some_and(|r| r.number == n)),
        "stale revision; read the card again"
    );
    r.revisions
        .iter_mut()
        .find(|r| r.number == n)
        .ok_or_else(|| anyhow::anyhow!("unknown revision"))
}

pub fn apply(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    if unchanged_save(w, actor, command, position, recorded_at)? {
        return Ok(());
    }
    apply_changed(w, actor, command, position, recorded_at)
}

/// `apply` for a live write that has checked `unchanged_save` and that drops `w` when the
/// command fails. A save writes into `w` without a copy of the workspace.
pub fn apply_checked(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    w.in_place = true;
    let result = apply_changed(w, actor, command, position, recorded_at);
    w.in_place = false;
    result
}

fn apply_changed(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    w.claim_time_ms = crate::tasks::parse_time(recorded_at);
    w.replaying = false;
    if let Some(op) = crate::removed_ops::name(command) {
        anyhow::bail!(crate::removed_ops::refusal(op));
    }
    if let Some(refusal) = crate::removed_ops::removed_field(command) {
        anyhow::bail!(refusal);
    }
    crate::decision_migration::check(w, command)?;
    crate::objects::validate_current(w, command)?;
    apply_inner(w, actor, command, position, recorded_at)?;
    // As in a replay, an event that writes no document skips the sync and the reset.
    if !writes_no_document(command) {
        crate::objects::sync(w, actor, command, position, recorded_at);
        crate::objects::reset_changed(w, command, position, recorded_at)?;
    }
    crate::source_reset::reset(w, command, position, recorded_at)
}

/// A live save_object that changes nothing succeeds without a revision or journal event.
pub fn unchanged_save(
    w: &Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<bool> {
    if !matches!(
        command,
        Command::SaveObject(crate::commands::SaveObjectOp { .. })
    ) {
        return Ok(false);
    }
    crate::objects::validate_current(w, command)?;
    identifier(actor)?;
    ensure!(w.members.contains_key(actor), "not a workspace member");
    crate::objects::unchanged_save(w, actor, command, position, recorded_at)
}

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

pub const REPLAY_RULES: [&str; 10] = [
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

/// The events that change only annotations, files, records, priorities, members
/// or workspace settings, or nothing.
fn writes_no_document(command: &Command) -> bool {
    matches!(
        command,
        Command::RecordReport(_)
            | Command::Note { .. }
            | Command::WithdrawNote { .. }
            | Command::Attach(_)
            | Command::Detach { .. }
            | Command::Assess(_)
            | Command::AssessSignal { .. }
            | Command::AcceptCommit { .. }
            | Command::SetPriority { .. }
            | Command::Member { .. }
            | Command::EnrollService { .. }
            | Command::RemoveMember { .. }
            | Command::UpdateWorkspace(_)
            | Command::RenameWorkspace(_)
    )
}

pub fn apply_replay(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    w.claim_time_ms = crate::tasks::parse_time(recorded_at);
    w.replaying = true;
    let result = apply_inner(w, actor, command, position, recorded_at);
    w.replaying = false;
    result?;
    replay_resets(w, actor, command, position, recorded_at)?;
    if !epoch_reached(w, SOURCE_CHANGE_RESET, position) {
        return Ok(());
    }
    crate::source_reset::reset(w, command, position, recorded_at)
}

/// The sync and the reset of a replayed event, as apply_changed gives them before the source
/// reset.
fn replay_resets(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    // An event that writes no document source and no object revision leaves the document
    // ledger and the criteria as the sync of the event before left them.
    if writes_no_document(command) {
        return Ok(());
    }
    crate::objects::sync(w, actor, command, position, recorded_at);
    // migrate_test_nodes, migrate_generic_nodes and migrate_decision_required change no status
    // of a criterion.
    if !epoch_reached(w, RESET_CHANGED, position)
        || matches!(
            command,
            Command::MigrateTestNodes {}
                | Command::MigrateGenericNodes {}
                | Command::MigrateDecisionRequired {}
        )
    {
        return Ok(());
    }
    crate::objects::reset_changed(w, command, position, recorded_at)
}
