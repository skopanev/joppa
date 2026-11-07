//! Arms of `apply_inner` in commands.rs, one function for each operation.
use super::*;

/// Apply `revise`.
pub(super) fn apply_revise(w: &mut Workspace, actor: &str, command: &Command) -> Result<()> {
    let Command::Revise(crate::commands::ReviseOp {
        req,
        expected_revision,
        capability,
        consumers,
        title,
        body,
        owner,
        reason,
        acs,
        coverage_policy,
        carry_tasks,
    }) = command
    else {
        unreachable!()
    };
    verification_identifier(w, req, "requirement")?;
    nonempty(title)?;
    nonempty(reason)?;
    human(w, owner)?;
    ensure!(
        capability
            .as_ref()
            .is_none_or(|id| w.capabilities.contains_key(id)),
        "unknown capability"
    );
    ensure!(
        capability
            .as_ref()
            .and_then(|id| w.capabilities.get(id))
            .and_then(|c| c.domain.as_ref().and_then(|id| w.domains.get(id)))
            .is_none_or(|d| !d.archived),
        "domain is archived"
    );
    ensure!(
        consumers.iter().collect::<BTreeSet<_>>().len() == consumers.len(),
        "duplicate consumer"
    );
    if !w.schema_migrated {
        for consumer in consumers {
            ensure!(w.consumers.contains_key(consumer), "unknown consumer");
        }
    }
    validate_acs(w, acs)?;
    // A criterion belongs to the current revision of one REQ. A moved criterion left the
    // current revision of its old REQ, and the older revisions keep it as history.
    for (other_id, other) in &w.requirements {
        if other_id != req {
            ensure!(
                !other
                    .revisions
                    .last()
                    .is_some_and(|r| r.acs.iter().any(|old| acs.iter().any(|a| a.id == old.id))),
                "AC id belongs to another REQ"
            );
        }
    }
    let r = w
        .requirements
        .entry(req.clone())
        .or_insert_with(|| Requirement {
            capability: capability.clone(),
            revisions: vec![],
            comments: BTreeMap::new(),
        });
    ensure!(
        r.revisions.last().map_or(0, |r| r.number) == *expected_revision,
        "stale revision; read the card again"
    );
    if let Some(old) = r.revisions.last() {
        ensure!(
            old.owner == *owner || actor == old.owner,
            "only current owner can transfer responsibility"
        );
    }
    r.capability = capability.clone();
    let sources = r
        .revisions
        .last()
        .map(|old| old.sources.clone())
        .unwrap_or_default();
    let prior_checks = r
        .revisions
        .last()
        .map(|old| {
            old.runs
                .values()
                .max_by_key(|run| run.started)
                .map(|run| run.plan.clone())
                .unwrap_or_else(|| old.prior_checks.clone())
        })
        .unwrap_or_default();
    let mut tasks = BTreeMap::new();
    if *carry_tasks {
        for old in &r.revisions {
            for (id, task) in &old.tasks {
                let mut task = task.clone();
                task.review_required = task.retired.is_none();
                if task.origin_revision == 0 {
                    task.origin_revision = old.number;
                }
                tasks.insert(id.clone(), task);
            }
        }
    }
    let acs: Vec<Ac> = acs.to_vec();
    r.revisions.push(Revision {
        number: expected_revision + 1,
        capability: capability.clone(),
        consumers: consumers.clone(),
        title: title.clone(),
        body: body.clone(),
        owner: owner.clone(),
        reason: reason.clone(),
        acs,
        confirmed: false,
        tasks,
        runs: BTreeMap::new(),
        issues: BTreeMap::new(),
        acceptance: None,
        coverage_policy: *coverage_policy,
        coverage_change: Vec::new(),
        prior_checks,
        coverage_review: None,
        verification_scope: None,
        sources,
    });
    Ok(())
}

/// Apply `link_source`.
pub(super) fn apply_link_source(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    recorded_at: &str,
) -> Result<()> {
    let Command::LinkSource(crate::commands::LinkSourceOp {
        req,
        revision: n,
        id,
        kind,
        uri,
        external_id,
        version,
        relation,
    }) = command
    else {
        unreachable!()
    };
    identifier(id)?;
    identifier(kind)?;
    ensure!(
        !uri.trim().is_empty() || external_id.as_ref().is_some_and(|id| !id.trim().is_empty()),
        "source needs a URI or an external identifier"
    );
    nonempty(version)?;
    ensure!(
        ["origin", "supports", "contradicts", "decision"].contains(&relation.as_str()),
        "unknown source relation"
    );
    let r = revision(w, req, *n, true)?;
    ensure!(!r.sources.contains_key(id), "source reference exists");
    r.sources.insert(
        id.clone(),
        SourceRef {
            kind: kind.clone(),
            uri: uri.clone(),
            external_id: external_id.clone(),
            version: version.clone(),
            relation: relation.clone(),
            actor: actor.to_owned(),
            at: recorded_at.to_owned(),
            withdrawn: false,
        },
    );
    Ok(())
}

/// Apply `withdraw_source`.
pub(super) fn apply_withdraw_source(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
) -> Result<()> {
    let Command::WithdrawSource(crate::commands::WithdrawSourceOp {
        req,
        revision: n,
        id,
        reason,
    }) = command
    else {
        unreachable!()
    };
    nonempty(reason)?;
    let r = revision(w, req, *n, true)?;
    let source = r
        .sources
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("unknown source reference"))?;
    ensure!(!source.withdrawn, "source reference already withdrawn");
    ensure!(
        actor == source.actor || actor == r.owner,
        "source author or REQ owner required"
    );
    source.withdrawn = true;
    Ok(())
}
