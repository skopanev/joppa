//! A criterion may move to another requirement. The move first gives the old requirement a
//! revision without the criterion, so each criterion belongs to one requirement revision and
//! the replayed documents name one parent.
use super::*;

/// The revision of the old requirement without the criterion that `cmd` moves to another one.
pub(crate) fn criterion_move_out(
    w: &Workspace,
    kind: &str,
    id: &str,
    cmd: &Command,
    reason: &str,
) -> Result<Option<Command>> {
    let Command::Revise(crate::commands::ReviseOp { req: to, .. }) = cmd else {
        return Ok(None);
    };
    let key = address(kind, id);
    let Some((from, r, ac)) = criterion_context(w, &key) else {
        return Ok(None);
    };
    if from == to {
        return Ok(None);
    }
    let (from, ac_id) = (from.to_owned(), ac.id.clone());
    let mut acs = r.acs.clone();
    acs.retain(|a| a.id != ac_id);
    let mut data = w.objects[&requirement_address(&from)]
        .current()
        .data
        .clone();
    if w.documents_migrated {
        data["consumers"] = json!([]);
    }
    let capability = (!w.documents_migrated)
        .then(|| w.requirements[&from].capability.clone())
        .flatten();
    let out = revise(&from, r, capability, &data, reason, acs)?;
    Ok(Some(out))
}
