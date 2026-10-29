//! Part of `objects`: `restore` and the items after it, moved from objects.rs unchanged.
use super::*;

pub fn restore(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
) -> Result<()> {
    let Command::RestoreObject {
        target,
        revision,
        reason,
    } = command
    else {
        bail!("restore_object required")
    };
    ensure!(may_edit_tree(w, actor), "workspace member required");
    commands::nonempty(reason)?;
    let target = resolve_address(w, target).to_owned();
    let old = w.objects.get(&target).context("unknown object")?;
    ensure!(
        old.current().revision == *revision,
        "stale object revision; read the object again"
    );
    ensure!(
        !old.current().present,
        "object is already present in the current tree"
    );
    ensure!(
        w.archived_objects.contains(&target),
        "object was not archived with archive_object"
    );

    let kind = &old.kind;
    ensure!(
        schema(w)
            .types
            .get(kind)
            .is_some_and(|t| ["group", "document"].contains(&t.role.as_str())),
        "restore_object requires a group or document"
    );

    parent_valid(
        &schema(w).types,
        w,
        kind,
        old.current().parent.as_deref(),
        Some(&target),
    )?;
    parent_cycle(w, &target, old.current().parent.as_deref())?;

    let mut next = old.current().clone();
    next.present = true;
    next.schema_revision = schema(w).revision;
    next.revision += 1;
    next.position = position;
    next.actor = actor.to_owned();
    next.reason = reason.clone();
    next.at = at.to_owned();

    w.objects.get_mut(&target).unwrap().revisions.push(next);
    w.archived_objects.remove(&target);
    Ok(())
}
