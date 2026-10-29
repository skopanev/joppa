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

/// Apply `assess`.
pub(super) fn apply_assess(w: &mut Workspace, command: &Command) -> Result<()> {
    let Command::Assess {
        req,
        revision: n,
        ac,
        assessment,
    } = command
    else {
        unreachable!()
    };
    nonempty(&assessment.reason)?;
    let q = w
        .requirements
        .get(req)
        .ok_or_else(|| anyhow::anyhow!("unknown REQ"))?;
    let r = q.revisions.last().unwrap();
    ensure!(r.number == *n, "stale revision");
    let a = r
        .acs
        .iter()
        .find(|a| &a.id == ac)
        .ok_or_else(|| anyhow::anyhow!("unknown AC"))?;
    if !assessment.ready {
        let recipient = assessment
            .owner
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("waiting needs a recipient"))?;
        ensure!(w.members.contains_key(recipient), "unknown recipient");
    }
    if assessment.ready && a.owner.is_some() {
        let decision = assessment
            .decision
            .as_ref()
            .and_then(|id| q.comments.get(id))
            .ok_or_else(|| anyhow::anyhow!("specialist decision must be linked"))?;
        let same_formulation = q
            .revisions
            .iter()
            .find(|source| source.number == decision.revision)
            .is_some_and(|source| {
                source.title == r.title
                    && source.body == r.body
                    && source.acs.iter().any(|old| {
                        old.id == a.id
                            && old.text == a.text
                            && old.owner == a.owner
                            && old.depends_on == a.depends_on
                            && old.before_launch == a.before_launch
                    })
            });
        ensure!(
            decision.kind == "decision"
                && !decision.withdrawn
                && Some(&decision.actor) == a.owner.as_ref()
                && decision.ac.as_ref() == Some(ac)
                && (decision.revision == *n || same_formulation),
            "decision must be by designated human for this AC and unchanged formulation"
        );
    } else if let Some(id) = &assessment.decision {
        ensure!(
            q.comments
                .get(id)
                .is_some_and(|c| c.kind == "decision" && !c.withdrawn),
            "unknown or withdrawn decision reference"
        );
    }
    let r = revision(w, req, *n, true)?;
    r.acs.iter_mut().find(|a| &a.id == ac).unwrap().assessment = Some(assessment.clone());
    r.acceptance = None;
    Ok(())
}
