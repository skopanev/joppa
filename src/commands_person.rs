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
    w.claim_time_ms = crate::tasks::parse_time(recorded_at);
    w.replay_claims = false;
    ensure!(
        !matches!(command, Command::HeartbeatTask { .. }),
        "heartbeat_task is no longer supported"
    );
    if let Some(op) = crate::removed_ops::name(command) {
        anyhow::bail!(crate::removed_ops::refusal(op));
    }
    crate::objects::validate_current(w, command)?;
    if let Command::ClaimTask { req, id, .. } = command {
        let key = crate::tasks::address(req, id);
        if crate::tasks::task(w, &key).is_some_and(|t| crate::tasks::holder(w, t).is_none()) {
            crate::tasks::ensure_agent_available(w, actor, &key)?;
        }
    }
    apply_inner(w, actor, command, position, recorded_at)?;
    crate::objects::sync(w, actor, command, position, recorded_at);
    crate::objects::reset_changed(w, position, recorded_at)
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

pub const KEEP_ASSESSMENTS: &str = "keep_assessments";

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

pub const REPLAY_RULES: [&str; 6] = [
    RESET_CHANGED,
    KEEP_ASSESSMENTS,
    LIST_SAVE_RESET,
    HUMAN_MEMBERS,
    SINGLE_CRITERION_SAVE,
    CRITERION_SAVE_REASON,
];

/// The workspace journal reached the epoch of the rule at `position`.
pub(crate) fn epoch_reached(w: &Workspace, rule: &str, position: u64) -> bool {
    w.replay_epochs
        .get(rule)
        .is_none_or(|first| position >= *first)
}

/// The rule applies to the event at `position`.
pub fn rule_applies(w: &Workspace, rule: &str, position: u64) -> bool {
    !w.replay_claims || epoch_reached(w, rule, position)
}

pub fn apply_replay(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    w.claim_time_ms = crate::tasks::parse_time(recorded_at);
    w.replay_claims = true;
    let result = apply_inner(w, actor, command, position, recorded_at);
    w.replay_claims = false;
    result?;
    crate::objects::sync(w, actor, command, position, recorded_at);
    if !epoch_reached(w, RESET_CHANGED, position) {
        return Ok(());
    }
    crate::objects::reset_changed(w, position, recorded_at)
}
