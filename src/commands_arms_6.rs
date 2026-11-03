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

/// Replay the older `assess` of a criterion in a requirement revision.
pub(super) fn apply_assess_criterion(w: &mut Workspace, op: &AssessCriterion) -> Result<()> {
    let r = revision(w, &op.req, op.revision, true)?;
    let ac = r
        .acs
        .iter_mut()
        .find(|a| a.id == op.ac)
        .context("unknown AC")?;
    ac.assessment = Some(op.assessment.clone());
    r.acceptance = None;
    Ok(())
}
