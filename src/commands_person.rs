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
    ensure!(is_human_member(w, actor), "a human member is required");
    Ok(())
}

/// The refusal of an agent or a runner on a command of the human members.
pub const HUMAN_MEMBER_REQUIRED: &str = "a human member of the workspace is required";

/// A member with role human. Each human member has the same rights, and the creator has no
/// other right.
pub fn is_human_member(w: &Workspace, actor: &str) -> bool {
    w.members.get(actor).is_some_and(|r| r == "human")
}

/// Refuse the actor unless it is a human member.
pub fn human_member(w: &Workspace, actor: &str) -> Result<()> {
    ensure!(is_human_member(w, actor), HUMAN_MEMBER_REQUIRED);
    Ok(())
}

/// Refuse the removal of `id` when no other human member stays.
pub fn keeps_human_member(w: &Workspace, id: &str) -> Result<()> {
    ensure!(
        w.members.iter().any(|(m, r)| m != id && r == "human"),
        "the last human member cannot be removed"
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
    if let Some(refusal) = crate::removed_ops::removed_field(command) {
        anyhow::bail!(refusal);
    }
    crate::objects::validate_current(w, command)?;
    identifier(actor)?;
    ensure!(w.members.contains_key(actor), "not a workspace member");
    crate::objects::unchanged_save(w, actor, command, position, recorded_at)
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
