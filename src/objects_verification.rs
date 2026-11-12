//! Part of `objects`: the input types, the verification verdict and the archive command.
use super::*;

pub fn input_type(w: &Workspace, kind: &str) -> bool {
    schema(w)
        .types
        .get(kind)
        .is_some_and(|t| t.inbox.unwrap_or(!w.documents_migrated && kind == "note"))
}

pub fn verification(w: &Workspace, key: &str) -> &'static str {
    let key = resolve_address(w, key);
    if !present_in_tree(w, key) {
        return "unverified";
    }
    if crate::tasks::task(w, key).is_some() {
        return crate::checks::task_verdict(w, key);
    }
    if let Some((req, r, ac)) = criterion_context(w, key) {
        let summary = requirement_summary(w, req, r);
        return match summary["acs"][&ac.id]["verification"].as_str() {
            Some("pass") => "pass",
            Some("fail") => "fail",
            _ => "unverified",
        };
    }
    if let Some(o) = w.objects.get(key)
        && schema(w)
            .types
            .get(&o.kind)
            .is_some_and(|t| t.role == "requirement")
        && let Some((req, r)) = verification_context(w, key)
    {
        let summary = requirement_summary(w, req, r);
        if summary["verified"] == true {
            return "pass";
        }
        if summary["acs"]
            .as_object()
            .is_some_and(|acs| acs.values().any(|a| a["verification"] == "fail"))
        {
            return "fail";
        }
    }
    "unverified"
}

pub fn archive(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
) -> Result<()> {
    let Command::ArchiveObject {
        target,
        revision,
        reason,
    } = command
    else {
        bail!("archive_object required")
    };
    let target = resolve_address(w, target).to_owned();
    ensure!(may_edit_tree(w, actor), "workspace member required");
    commands::nonempty(reason)?;
    let old = w.objects.get(&target).context("unknown object")?;
    ensure!(
        old.current().revision == *revision,
        "stale object revision; read the object again"
    );
    ensure!(
        old.current().present,
        "object is absent from the current tree"
    );
    let kind = &old.kind;
    let has_live_children = w
        .objects
        .values()
        .any(|o| o.current().parent.as_ref() == Some(&target) && o.current().present);
    // A stored archive passed this check when its done and retired Tasks did not count.
    ensure!(
        w.replaying || !has_live_children,
        "cannot archive an object with active children"
    );

    let mut next = old.current().clone();
    next.present = false;
    next.schema_revision = schema(w).revision;
    next.revision += 1;
    next.position = position;
    next.actor = actor.to_owned();
    next.reason = reason.clone();
    next.at = at.to_owned();

    let parent = next.parent.clone();
    let inbox = input_type(w, kind);
    w.objects.get_mut(&target).unwrap().revisions.push(next);
    w.archived_objects.insert(target.clone());
    if !commands::rule_applies(w, commands::LIST_SAVE_RESET, position) {
        return Ok(());
    }
    // An archived spec object resets the criteria at and below itself. Before the epoch of
    // archive_own_reset, it reset them at and below its parent.
    let root = match commands::rule_applies(w, commands::ARCHIVE_OWN_RESET, position) {
        true => Some(target),
        false => parent,
    };
    match root {
        Some(root) if !inbox => reset_criteria(w, &root, at),
        _ => Ok(()),
    }
}
