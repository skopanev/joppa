//! Arms of `apply_inner` in commands.rs, one function for each operation.
use super::*;

/// Apply `rename_workspace`.
pub(super) fn apply_rename_workspace(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
) -> Result<()> {
    let Command::RenameWorkspace(crate::commands::RenameWorkspaceOp {
        workspace_id,
        title,
        reason,
        reference_prefix,
    }) = command
    else {
        unreachable!()
    };
    ensure!(actor == w.admin, "workspace owner renames the workspace");
    nonempty(title)?;
    nonempty(reason)?;
    w.title = title.clone();
    if let Some(id) = workspace_id {
        identifier(id)?;
        w.workspace_id = Some(id.clone());
    }
    if let Some(prefix) = reference_prefix {
        identifier(prefix)?;
        w.reference_prefix = Some(prefix.clone());
        // The server derives a prefix from the title with this reason. An explicit
        // prefix stays until the next rename without one.
        w.prefix_from_title = reason == crate::store::workspace_prefix::REASON;
    }
    Ok(())
}

/// Apply `member`.
pub(super) fn apply_member(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
) -> Result<()> {
    let Command::Member { id, role } = command else {
        unreachable!()
    };
    ensure!(actor == w.admin, "workspace owner required");
    identifier(id)?;
    // From its epoch on, a membership is a person. An older event keeps its role.
    let roles: &[&str] = if rule_applies(w, HUMAN_MEMBERS, position) {
        &["human"]
    } else {
        &["human", "agent", "runner"]
    };
    ensure!(roles.contains(&role.as_str()), "unknown role");
    ensure!(
        !w.members.contains_key(id),
        "member exists; identities cannot be reassigned"
    );
    w.members.insert(id.clone(), role.clone());
    Ok(())
}

/// Apply `enroll_service`.
pub(super) fn apply_enroll_service(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
) -> Result<()> {
    let Command::EnrollService { id, role } = command else {
        unreachable!()
    };
    ensure!(actor == w.admin, "workspace owner required");
    identifier(id)?;
    ensure!(["agent", "runner"].contains(&role.as_str()), "unknown role");
    ensure!(
        !w.members.contains_key(id),
        "member exists; identities cannot be reassigned"
    );
    w.members.insert(id.clone(), role.clone());
    Ok(())
}

/// Apply `consumer`.
pub(super) fn apply_consumer(w: &mut Workspace, command: &Command) -> Result<()> {
    let Command::Consumer(crate::commands::ConsumerOp {
        id,
        title,
        description,
        owner,
    }) = command
    else {
        unreachable!()
    };
    identifier(id)?;
    nonempty(title)?;
    human(w, owner)?;
    ensure!(!w.consumers.contains_key(id), "consumer exists");
    w.consumers.insert(
        id.clone(),
        Consumer {
            title: title.clone(),
            description: description.clone(),
            owner: owner.clone(),
        },
    );
    Ok(())
}

/// Apply `edit_consumer`.
pub(super) fn apply_edit_consumer(w: &mut Workspace, actor: &str, command: &Command) -> Result<()> {
    let Command::EditConsumer(crate::commands::EditConsumerOp {
        id,
        title,
        description,
        owner,
        reason,
    }) = command
    else {
        unreachable!()
    };
    nonempty(title)?;
    nonempty(reason)?;
    human(w, owner)?;
    let consumer = w
        .consumers
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("unknown consumer"))?;
    ensure!(
        actor == w.admin || actor == consumer.owner,
        "consumer owner required"
    );
    consumer.title = title.clone();
    consumer.description = description.clone();
    consumer.owner = owner.clone();
    Ok(())
}

/// Apply `domain`.
pub(super) fn apply_domain(w: &mut Workspace, actor: &str, command: &Command) -> Result<()> {
    let Command::Domain(crate::commands::DomainOp {
        id,
        title,
        owner,
        description,
    }) = command
    else {
        unreachable!()
    };
    identifier(id)?;
    nonempty(title)?;
    human(w, owner)?;
    ensure!(!w.domains.contains_key(id), "domain exists");
    w.domains.insert(
        id.clone(),
        Domain {
            archived: false,
            title: title.clone(),
            owner: owner.clone(),
            description: description.clone(),
            confirmed: actor == w.admin,
        },
    );
    Ok(())
}

/// Apply `capability`.
pub(super) fn apply_capability(w: &mut Workspace, actor: &str, command: &Command) -> Result<()> {
    let Command::Capability(crate::commands::CapabilityOp {
        id,
        title,
        domain,
        owner,
        description,
    }) = command
    else {
        unreachable!()
    };
    identifier(id)?;
    nonempty(title)?;
    if let Some(domain) = domain {
        ensure!(w.domains.contains_key(domain), "unknown domain");
        ensure!(!w.domains[domain].archived, "domain is archived");
    }
    ensure!(!w.capabilities.contains_key(id), "capability exists");
    let assigned = owner.as_deref().unwrap_or_else(|| {
        domain
            .as_ref()
            .and_then(|id| w.domains.get(id))
            .map_or(w.admin.as_str(), |d| d.owner.as_str())
    });
    human(w, assigned)?;
    w.capabilities.insert(
        id.clone(),
        Capability {
            title: title.clone(),
            domain: domain.clone(),
            owner: Some(assigned.to_owned()),
            description: description.clone(),
            confirmed: actor == w.admin,
        },
    );
    Ok(())
}

/// Apply `set_domain_archived`.
pub(super) fn apply_set_domain_archived(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
) -> Result<()> {
    let Command::SetDomainArchived(crate::commands::SetDomainArchivedOp {
        id,
        archived,
        reason,
    }) = command
    else {
        unreachable!()
    };
    ensure!(actor == w.admin, "workspace owner archives domains");
    nonempty(reason)?;
    let domain = w
        .domains
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("unknown domain"))?;
    ensure!(domain.archived != *archived, "archive state is unchanged");
    if *archived {
        ensure!(
            !w.requirements.values().any(|r| r
                .capability
                .as_ref()
                .and_then(|cap| w.capabilities.get(cap))
                .is_some_and(|c| c.domain.as_deref() == Some(id))),
            "only domains without requirements can be archived"
        );
    }
    w.domains.get_mut(id).unwrap().archived = *archived;
    Ok(())
}
