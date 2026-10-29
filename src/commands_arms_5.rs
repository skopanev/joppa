//! Arms of `apply_inner` in commands.rs, one function for each operation.
use super::*;

/// Apply `confirm`.
pub(super) fn apply_confirm(w: &mut Workspace, actor: &str, command: &Command) -> Result<()> {
    let Command::Confirm { req, revision: n } = command else {
        unreachable!()
    };
    let q = w
        .requirements
        .get(req)
        .ok_or_else(|| anyhow::anyhow!("unknown REQ"))?;
    ensure!(
        q.capability
            .as_ref()
            .and_then(|id| w.capabilities.get(id))
            .is_none_or(|c| c.confirmed
                && c.domain
                    .as_ref()
                    .and_then(|id| w.domains.get(id))
                    .is_none_or(|d| d.confirmed)),
        "workspace owner must confirm the business tree first"
    );
    let r = revision(w, req, *n, true)?;
    ensure!(r.owner == actor, "REQ owner confirms its formulation");
    r.confirmed = true;
    Ok(())
}

/// Apply `describe`.
pub(super) fn apply_describe(w: &mut Workspace, command: &Command) -> Result<()> {
    let Command::Describe(crate::commands::DescribeOp {
        target: t,
        text,
        reason,
    }) = command
    else {
        unreachable!()
    };
    target(w, t)?;
    nonempty(reason)?;
    ensure!(
        text.chars().count() <= DESCRIPTION_MAX,
        "a description is at most {DESCRIPTION_MAX} characters"
    );
    // Old journal events keep embedded descriptions. Current objects use common annotations.
    if !w.documents_migrated
        && let Some(domain) = t
            .strip_prefix("domain:")
            .and_then(|id| w.domains.get_mut(id))
    {
        domain.description = text.clone();
    } else if !w.documents_migrated
        && let Some(capability) = t
            .strip_prefix("capability:")
            .and_then(|id| w.capabilities.get_mut(id))
    {
        capability.description = text.clone();
    } else {
        w.notes
            .entry(crate::objects::annotation_address(w, t))
            .or_default()
            .description = text.clone();
    }
    Ok(())
}

/// Apply `note`.
pub(super) fn apply_note(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    recorded_at: &str,
) -> Result<()> {
    let Command::Note {
        target: t,
        id,
        text,
        reply_to,
    } = command
    else {
        unreachable!()
    };
    target(w, t)?;
    identifier(id)?;
    nonempty(text)?;
    let notes = w
        .notes
        .entry(crate::objects::annotation_address(w, t))
        .or_default();
    ensure!(!notes.comments.contains_key(id), "comment {id} exists");
    if let Some(parent) = reply_to {
        ensure!(
            notes.comments.contains_key(parent),
            "no such comment to reply to: {parent}"
        );
    }
    notes.comments.insert(
        id.clone(),
        Note {
            actor: actor.into(),
            at: recorded_at.to_string(),
            text: text.clone(),
            reply_to: reply_to.clone(),
            withdrawn: false,
        },
    );
    Ok(())
}

/// Apply `withdraw_note`.
pub(super) fn apply_withdraw_note(w: &mut Workspace, actor: &str, command: &Command) -> Result<()> {
    let Command::WithdrawNote {
        target: t,
        id,
        reason,
    } = command
    else {
        unreachable!()
    };
    target(w, t)?;
    nonempty(reason)?;
    let annotation = crate::objects::annotation_address(w, t);
    let note = w
        .notes
        .get_mut(&annotation)
        .and_then(|n| n.comments.get_mut(id))
        .ok_or_else(|| anyhow::anyhow!("no such comment: {id}"))?;
    ensure!(note.actor == actor, "a comment is withdrawn by its author");
    note.withdrawn = true;
    Ok(())
}

/// Apply `attach`.
pub(super) fn apply_attach(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    recorded_at: &str,
) -> Result<()> {
    let Command::Attach(crate::commands::AttachOp {
        target: t,
        id,
        name,
        content_type,
        size,
        hash,
    }) = command
    else {
        unreachable!()
    };
    target(w, t)?;
    identifier(id)?;
    nonempty(name)?;
    ensure!(*size > 0, "an empty attachment carries nothing");
    // Именно НИЖНИЙ регистр: хранилище кладёт файл под ключом из
    // {:x}, и тот же отпечаток заглавными буквами прошёл бы проверку,
    // но не нашёл бы файла — вложение было бы записано и не
    // открывалось бы.
    ensure!(
        hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "the attachment digest is a sha256 in lowercase hex"
    );
    let notes = w
        .notes
        .entry(crate::objects::annotation_address(w, t))
        .or_default();
    ensure!(
        !notes.attachments.contains_key(id),
        "attachment {id} exists"
    );
    notes.attachments.insert(
        id.clone(),
        Attachment {
            name: name.clone(),
            content_type: if content_type.is_empty() {
                "application/octet-stream".into()
            } else {
                content_type.clone()
            },
            size: *size,
            hash: hash.clone(),
            actor: actor.into(),
            at: recorded_at.to_string(),
            removed: false,
        },
    );
    Ok(())
}

/// Apply `detach`.
pub(super) fn apply_detach(w: &mut Workspace, command: &Command) -> Result<()> {
    let Command::Detach {
        target: t,
        id,
        reason,
    } = command
    else {
        unreachable!()
    };
    target(w, t)?;
    nonempty(reason)?;
    let annotation = crate::objects::annotation_address(w, t);
    let a = w
        .notes
        .get_mut(&annotation)
        .and_then(|n| n.attachments.get_mut(id))
        .ok_or_else(|| anyhow::anyhow!("no such attachment: {id}"))?;
    a.removed = true;
    Ok(())
}
