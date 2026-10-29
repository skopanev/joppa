//! Part of `commands`: `apply_first`, moved from commands.rs unchanged.
use super::*;

/// Apply the operations that need no workspace member: enrollment, a reference removal, the
/// workspace creation and a record report, and the planning and Task operations. Gives true
/// when one of them applied the command.
pub(super) fn apply_first(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<bool> {
    identifier(actor)?;
    if let Command::EnrollMember { id } = command {
        ensure!(
            actor == "joppa-access" && !w.admin.is_empty() && w.deleted_at.is_none(),
            "internal enrollment required"
        );
        identifier(id)?;
        ensure!(
            id != "joppa-access" && !w.members.contains_key(id),
            "invalid enrollment identity"
        );
        w.members.insert(id.clone(), "human".into());
        return Ok(true);
    }
    // The internal actor of a deletion is no member; the removal checks the actor itself.
    if let Command::RemoveWorkspaceReferences { workspace, reason } = command {
        return crate::workspace_references::remove(
            w,
            actor,
            workspace,
            reason,
            position,
            recorded_at,
        )
        .map(|()| true);
    }
    if let Command::Workspace { title } = command {
        ensure!(w.admin.is_empty(), "workspace exists");
        nonempty(title)?;
        w.title = title.clone();
        w.admin = actor.into();
        w.members.insert(actor.into(), "human".into());
        for (kind, fields) in [
            ("bug", vec!["observed", "expected", "where"]),
            ("policy", vec!["rule", "authority", "applies_to"]),
            ("question", vec!["unknown", "decides"]),
            ("idea", vec!["outcome", "today"]),
        ] {
            w.questions
                .insert(kind.into(), fields.into_iter().map(String::from).collect());
        }
        return Ok(true);
    }
    ensure!(w.members.contains_key(actor), "not a workspace member");
    if let Command::RecordReport(crate::commands::RecordReportOp { .. }) = command {
        return Ok(true);
    }
    if crate::planning::apply(w, actor, command, position, recorded_at)? {
        return Ok(true);
    }
    if crate::tasks::apply(w, actor, command, position, recorded_at)? {
        return Ok(true);
    }
    Ok(false)
}
