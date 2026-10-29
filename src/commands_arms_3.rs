//! Arms of `apply_inner` in commands.rs, one function for each operation.
use super::*;

/// Apply `confirm_tree`.
pub(super) fn apply_confirm_tree(w: &mut Workspace, actor: &str) -> Result<()> {
    ensure!(actor == w.admin, "workspace owner confirms the tree");
    for d in w.domains.values_mut() {
        d.confirmed = true;
    }
    for c in w.capabilities.values_mut() {
        c.confirmed = true;
    }
    Ok(())
}

/// Apply `edit_domain`.
pub(super) fn apply_edit_domain(w: &mut Workspace, actor: &str, command: &Command) -> Result<()> {
    let Command::EditDomain(crate::commands::EditDomainOp {
        id,
        title,
        owner,
        description,
        reason,
    }) = command
    else {
        unreachable!()
    };
    let domain = w
        .domains
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("unknown domain"))?;
    ensure!(
        actor == w.admin
            || (w.members[actor] == "agent"
                && !domain.confirmed
                && !domain.archived
                && domain.owner == *owner),
        "only the workspace owner edits confirmed domains or changes authority; agents may edit draft titles"
    );
    nonempty(title)?;
    nonempty(reason)?;
    human(w, owner)?;
    let d = w
        .domains
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("unknown domain"))?;
    d.title = title.clone();
    d.owner = owner.clone();
    d.description = description.clone();
    Ok(())
}

/// Apply `edit_capability`.
pub(super) fn apply_edit_capability(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
) -> Result<()> {
    let Command::EditCapability(crate::commands::EditCapabilityOp {
        id,
        title,
        owner,
        description,
        reason,
    }) = command
    else {
        unreachable!()
    };
    let capability = w
        .capabilities
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("unknown capability"))?;
    ensure!(
        actor == w.admin
            || (w.members[actor] == "agent"
                && !capability.confirmed
                && capability
                    .domain
                    .as_ref()
                    .and_then(|id| w.domains.get(id))
                    .is_none_or(|d| !d.archived)
                && owner
                    .as_ref()
                    .is_none_or(|requested| capability.owner.as_ref() == Some(requested))),
        "only the workspace owner edits confirmed capabilities; agents may edit draft titles"
    );
    nonempty(title)?;
    nonempty(reason)?;
    if let Some(owner) = owner {
        human(w, owner)?;
    }
    let cap = w
        .capabilities
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("unknown capability"))?;
    cap.title = title.clone();
    if let Some(owner) = owner {
        cap.owner = Some(owner.clone());
    }
    cap.description = description.clone();
    Ok(())
}

/// Apply `withdraw_decision`.
pub(super) fn apply_withdraw_decision(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
) -> Result<()> {
    let Command::WithdrawDecision(crate::commands::WithdrawDecisionOp {
        req,
        revision: n,
        comment,
        reason,
    }) = command
    else {
        unreachable!()
    };
    nonempty(reason)?;
    let q = w
        .requirements
        .get_mut(req)
        .ok_or_else(|| anyhow::anyhow!("unknown REQ"))?;
    let c = q
        .comments
        .get_mut(comment)
        .ok_or_else(|| anyhow::anyhow!("unknown decision"))?;
    ensure!(
        c.kind == "decision" && c.actor == actor && c.revision == *n && !c.withdrawn,
        "only author may withdraw their active decision"
    );
    c.withdrawn = true;
    for r in &mut q.revisions {
        for a in &mut r.acs {
            if a.assessment
                .as_ref()
                .is_some_and(|s| s.decision.as_ref() == Some(comment))
            {
                a.assessment = Some(Assessment {
                    ready: false,
                    reason: reason.clone(),
                    owner: Some(actor.into()),
                    decision: Some(comment.clone()),
                });
                r.acceptance = None;
            }
        }
    }
    Ok(())
}
