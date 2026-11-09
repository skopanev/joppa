//! Part of `objects`: `ancestor` and the items after it, moved from objects.rs unchanged.
use super::*;

pub(crate) fn ancestor(w: &Workspace, parent: Option<&str>, kind: &str) -> Result<String> {
    let mut p = parent;
    let mut seen = BTreeSet::new();
    while let Some(key) = p {
        ensure!(seen.insert(key), "tree parent cycle");
        let o = w
            .objects
            .get(resolve_address(w, key))
            .context("unknown ancestor")?;
        if o.kind == kind {
            return Ok(o.id.clone());
        }
        p = o.current().parent.as_deref();
    }
    bail!("semantic {kind} ancestor is required")
}

pub fn save(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
) -> Result<()> {
    let old = saved_current(w, command);
    // A replay stops at any refused event, and a live write drops the state of a refused
    // command. So both write into the state without a copy.
    if w.replaying || w.in_place {
        if save_into(w, actor, command, position, at, false)? {
            mark_contract(old.as_ref(), w, command);
        }
        return Ok(());
    }
    if let Some(mut candidate) = saved(w, actor, command, position, at, false)? {
        mark_contract(old.as_ref(), &mut candidate, command);
        *w = candidate;
    }
    Ok(())
}

/// The current revision of the object that a save_object names, before the save.
fn saved_current(w: &Workspace, command: &Command) -> Option<ObjectRevision> {
    let Command::SaveObject(crate::commands::SaveObjectOp { kind, id, .. }) = command else {
        return None;
    };
    let key = address(kind, id);
    Some(w.objects.get(resolve_address(w, &key))?.current().clone())
}

/// A new revision that changes only the test list or only flow keys keeps the contract
/// revision of the revision before it.
pub(crate) fn mark_contract(
    old: Option<&ObjectRevision>,
    candidate: &mut Workspace,
    command: &Command,
) {
    let Command::SaveObject(crate::commands::SaveObjectOp { kind, id, .. }) = command else {
        return;
    };
    let key = address(kind, id);
    let Some(old) = old else {
        return;
    };
    let target = resolve_address(candidate, &key).to_owned();
    let Some(new) = candidate
        .objects
        .get_mut(&target)
        .and_then(|o| o.revisions.last_mut())
    else {
        return;
    };
    if new.revision != old.revision && (tests_only_change(old, new) || flow_only_change(old, new)) {
        new.contract = Some([new.revision, contract_of(old)]);
    }
}

/// A live save whose normalized data, parent and links equal the current revision changes nothing.
pub fn unchanged_save(
    w: &Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
) -> Result<bool> {
    Ok(saved(w, actor, command, position, at, true)?.is_none())
}
