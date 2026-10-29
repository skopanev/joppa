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
    if let Some(mut candidate) = saved(w, actor, command, position, at, false)? {
        mark_contract(w, &mut candidate, command);
        *w = candidate;
    }
    Ok(())
}

/// A new revision that changes only the test list keeps the contract revision of the
/// revision before it.
pub(crate) fn mark_contract(w: &Workspace, candidate: &mut Workspace, command: &Command) {
    let Command::SaveObject(crate::commands::SaveObjectOp { kind, id, .. }) = command else {
        return;
    };
    let key = address(kind, id);
    let Some(old) = w.objects.get(resolve_address(w, &key)).map(Object::current) else {
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
    if new.revision != old.revision && tests_only_change(old, new) {
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
