//! Arms of `apply_inner` in commands.rs, one function for each operation.
use super::*;

/// Apply `comment`.
pub(super) fn apply_comment(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    recorded_at: &str,
) -> Result<()> {
    let Command::Comment(crate::commands::CommentOp {
        req,
        revision: n,
        id,
        text,
        kind,
        reply_to,
        ac,
    }) = command
    else {
        unreachable!()
    };
    identifier(id)?;
    nonempty(text)?;
    ensure!(
        [
            "comment",
            "decision",
            "accept",
            "change",
            "reject",
            "correction"
        ]
        .contains(&kind.as_str()),
        "unknown comment kind"
    );
    if ["decision", "accept", "reject", "change"].contains(&kind.as_str()) {
        person(w, actor)?;
    }
    let r = revision(w, req, *n, false)?;
    if ["accept", "change", "reject"].contains(&kind.as_str()) {
        ensure!(r.owner == actor, "owner replies to suggestions");
    }
    if let Some(a) = ac {
        ensure!(r.acs.iter().any(|c| &c.id == a), "unknown AC");
    }
    let q = w.requirements.get_mut(req).unwrap();
    ensure!(!q.comments.contains_key(id), "comment id exists");
    if let Some(p) = reply_to {
        ensure!(q.comments.contains_key(p), "unknown previous comment");
    }
    if kind == "correction" {
        ensure!(
            reply_to.is_some(),
            "correction must reference prior comment"
        );
    }
    q.comments.insert(
        id.clone(),
        Comment {
            revision: *n,
            actor: actor.into(),
            // Из журнала, не от клиента: по этим отметкам потом
            // восстанавливают, кто что знал и когда.
            at: recorded_at.to_string(),
            text: text.clone(),
            kind: kind.clone(),
            reply_to: reply_to.clone(),
            ac: ac.clone(),
            withdrawn: false,
        },
    );
    Ok(())
}

/// Apply `update_workspace`. An old `rename_workspace` applies as one with its fields.
pub(super) fn apply_update_workspace(
    w: &mut Workspace,
    actor: &str,
    op: &crate::commands::UpdateWorkspaceOp,
) -> Result<()> {
    let crate::commands::UpdateWorkspaceOp {
        title,
        workspace_id,
        reference_prefix,
        hidden,
        reason,
    } = op;
    let owner_fields = title.is_some() || workspace_id.is_some() || reference_prefix.is_some();
    ensure!(
        owner_fields || hidden.is_some(),
        "update_workspace needs title, workspace_id, reference_prefix or hidden"
    );
    // hidden alone is the flag of the sender, so any member may send it.
    ensure!(
        !owner_fields || actor == w.admin,
        "workspace owner updates the workspace"
    );
    for text in [title, reason].into_iter().flatten() {
        nonempty(text)?;
    }
    for id in [workspace_id, reference_prefix].into_iter().flatten() {
        identifier(id)?;
    }
    if let Some(title) = title {
        w.title = title.clone();
    }
    if let Some(id) = workspace_id {
        w.workspace_id = Some(id.clone());
    }
    if let Some(prefix) = reference_prefix {
        w.reference_prefix = Some(prefix.clone());
        // The server derives a prefix from the title with this reason. After an explicit
        // prefix, it derives none.
        w.prefix_from_title = reason.as_deref() == Some(crate::store::workspace_prefix::REASON);
    }
    match hidden {
        Some(true) => _ = w.hidden_by.insert(actor.to_owned()),
        Some(false) => _ = w.hidden_by.remove(actor),
        None => {}
    }
    Ok(())
}
