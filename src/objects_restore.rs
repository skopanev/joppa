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

    let kind = old.kind.clone();
    let role = schema(w)
        .types
        .get(&kind)
        .context("unknown object type")?
        .role
        .clone();

    parent_valid(
        &schema(w).types,
        w,
        &kind,
        old.current().parent.as_deref(),
        Some(&target),
    )?;
    parent_cycle(w, &target, old.current().parent.as_deref())?;
    crate::relations::check_restore(w, &old.current().relations)?;

    let mut next = old.current().clone();
    next.present = true;
    next.schema_revision = schema(w).revision;
    next.revision += 1;
    next.position = position;
    next.actor = actor.to_owned();
    next.reason = reason.clone();
    next.at = at.to_owned();
    if commands::rule_applies(w, commands::OWN_ALIAS, position) {
        next.alias = w.alias.clone();
    }

    let restored_owner = next.data["owner"].as_str().map(str::to_owned);
    std::sync::Arc::make_mut(w.objects.get_mut(&target).unwrap())
        .revisions
        .push(next);
    std::sync::Arc::make_mut(&mut w.archived_objects).remove(&target);
    if role == "ac" && !w.document_overrides.contains(&target) {
        align_restored_criterion(w, &target, restored_owner.as_deref());
    }
    if !["group", "document"].contains(&role.as_str()) && !input_type(w, &kind) {
        reset_criteria(w, &target, at)?;
    }
    Ok(())
}

/// Keep a restored criterion aligned with `w.requirements` when its parent requirement changed
/// owner while the criterion was archived.
fn align_restored_criterion(w: &mut Workspace, target: &str, owner: Option<&str>) {
    let found = criterion_context(w, target).map(|(req, _, ac)| (req.to_owned(), ac.id.clone()));
    let Some((req, ac_id)) = found else {
        std::sync::Arc::make_mut(&mut w.document_overrides).insert(target.to_owned());
        return;
    };
    let Some(owner) = owner else {
        return;
    };
    if let Some(q) = w.requirements.get_mut(&req)
        && let Some(last) = q.revisions.last()
        && last
            .acs
            .iter()
            .any(|a| a.id == ac_id && a.owner.as_deref().unwrap_or(&last.owner) != owner)
    {
        let mut next_r = last.clone();
        next_r.number += 1;
        if let Some(ac) = next_r.acs.iter_mut().find(|a| a.id == ac_id) {
            ac.owner = Some(owner.to_owned());
        }
        std::sync::Arc::make_mut(q).revisions.push(next_r);
    }
}
