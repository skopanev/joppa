//! Arms of `apply_inner` in commands.rs, one function for each operation.
use super::*;

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
    human_member(w, actor)?;
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
    // A server-only operation: the creator record is an internal check, not a right.
    ensure!(
        actor == w.admin,
        "only the workspace creator enrolls a service identity through the server"
    );
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
    // The owner of a consumer is a human member too.
    ensure!(
        w.members.get(actor).is_some_and(|r| r == "human"),
        HUMAN_MEMBER_REQUIRED
    );
    consumer.title = title.clone();
    consumer.description = description.clone();
    consumer.owner = owner.clone();
    Ok(())
}

/// Apply `domain`.
pub(super) fn apply_domain(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
) -> Result<()> {
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
    let confirmed = confirms(w, actor, position);
    w.domains.insert(
        id.clone(),
        Domain {
            archived: false,
            title: title.clone(),
            owner: owner.clone(),
            description: description.clone(),
            confirmed,
        },
    );
    Ok(())
}

/// A new domain or capability is confirmed when a human member saves it. Before the epoch of
/// human_member_rights, only the creator of the workspace confirmed it in a replay.
fn confirms(w: &Workspace, actor: &str, position: u64) -> bool {
    if rule_applies(w, HUMAN_MEMBER_RIGHTS, position) {
        is_human_member(w, actor)
    } else {
        actor == w.admin
    }
}

/// Apply `capability`.
pub(super) fn apply_capability(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
) -> Result<()> {
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
    // From the epoch of human_member_rights, the default owner is the actor when it is a human
    // member, else the system actor. An older event keeps the creator in a replay.
    let fallback = if rule_applies(w, HUMAN_MEMBER_RIGHTS, position) {
        default_owner(w, actor)
    } else {
        w.admin.as_str()
    };
    let assigned = owner
        .as_deref()
        .unwrap_or_else(|| {
            domain
                .as_ref()
                .and_then(|id| w.domains.get(id))
                .map_or(fallback, |d| d.owner.as_str())
        })
        .to_owned();
    human(w, &assigned)?;
    let confirmed = confirms(w, actor, position);
    w.capabilities.insert(
        id.clone(),
        Capability {
            title: title.clone(),
            domain: domain.clone(),
            owner: Some(assigned),
            description: description.clone(),
            confirmed,
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
    human_member(w, actor)?;
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
