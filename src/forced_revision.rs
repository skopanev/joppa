//! The rule of forced bumps (requirement:edit-makes-revision). A save, an archive or a restore
//! of a criterion gives its requirement a forced revision with the same data, parent, relations
//! and presence. A later save_object of that requirement in the same write may send the revision
//! that it read before the write. Each revision above the sent one must be such a forced bump of
//! an earlier operation of the write. The server then writes the current revision into the
//! command, so the save checks it as today and the journal stores a current revision. Any other
//! gap stays stale. A joppa_command has one operation, so only a joppa_batch uses the rule.
use crate::commands::Command;
use crate::model::Workspace;
use crate::objects::{ObjectRevision, address, resolve_address, schema};
use crate::store::Event;
use serde_json::{Value, json};

/// Write the current revision into a live save_object of a requirement when the gap from the
/// sent revision has only forced bumps of `events`, the earlier operations of this write.
pub fn catch_up(w: &Workspace, value: &mut Value, base_position: u64, events: &[Event]) {
    if value["op"] != "save_object" {
        return;
    }
    let (Some(kind), Some(id)) = (value["kind"].as_str(), value["id"].as_str()) else {
        return;
    };
    let sent = value["revision"].as_u64().unwrap_or(0);
    let Some(object) = w.objects.get(resolve_address(w, &address(kind, id))) else {
        return;
    };
    if sent == 0 || sent >= u64::from(object.current().revision) {
        return;
    }
    let types = schema(w).types;
    let role = |kind: &str| types.get(kind).map(|t| t.role.as_str());
    if role(kind) != Some("requirement") {
        return;
    }
    // The operation at the position saves, archives or restores an object with the role ac.
    let criterion_write = |position: u64| {
        let event = events.iter().find(|e| e.position == position);
        position > base_position
            && event.is_some_and(|e| match &e.command {
                Command::SaveObject(op) => role(&op.kind) == Some("ac"),
                Command::ArchiveObject { target, .. } | Command::RestoreObject { target, .. } => {
                    let target = w.objects.get(resolve_address(w, target));
                    target.is_some_and(|o| role(&o.kind) == Some("ac"))
                }
                _ => false,
            })
    };
    if let Some(current) = caught_up(&object.revisions, sent, criterion_write) {
        value["revision"] = json!(current);
    }
}

/// The current revision when each revision above `sent` is a forced bump: `forced` accepts its
/// position, and it has the data, the parent, the relations and the presence of the revision
/// before it. None for any other gap.
pub(crate) fn caught_up(
    revisions: &[ObjectRevision],
    sent: u64,
    forced: impl Fn(u64) -> bool,
) -> Option<u32> {
    let current = revisions.last()?.revision;
    let first = revisions
        .iter()
        .position(|r| u64::from(r.revision) > sent)?;
    // The sent revision must stay in memory: it was current at the start of the write.
    let before = first.checked_sub(1).map(|i| &revisions[i])?;
    if u64::from(before.revision) != sent {
        return None;
    }
    let same = revisions[first - 1..].windows(2).all(|pair| {
        let (before, next) = (&pair[0], &pair[1]);
        forced(next.position)
            && next.data == before.data
            && next.parent == before.parent
            && next.relations == before.relations
            && next.present == before.present
    });
    same.then_some(current)
}

#[cfg(test)]
#[path = "forced_revision_tests.rs"]
mod tests;
