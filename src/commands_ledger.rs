//! Part of `commands`: the ledger inputs before a live save or archive, so the write syncs
//! only the requirements that it changes (objects_sync_scope.rs). Any other operation syncs
//! every row.
use super::*;

/// The snapshot before `command`, or None when the write syncs every row.
pub(super) fn snapshot(w: &Workspace, command: &Command) -> Option<crate::objects::Snapshot> {
    let own = match command {
        Command::SaveObject(crate::commands::SaveObjectOp { kind, id, .. }) => {
            crate::objects::address(kind, id)
        }
        Command::ArchiveObject { target, .. } | Command::RestoreObject { target, .. } => {
            target.clone()
        }
        _ => return None,
    };
    let own = crate::objects::resolve_address(w, &own).to_owned();
    Some(crate::objects::Snapshot::of(w, Some(&own)))
}
