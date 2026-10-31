//! Part of `commands`: `apply_inner`, moved from commands.rs unchanged.
use super::*;

pub(crate) fn apply_inner(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    if apply_first(w, actor, command, position, recorded_at)? {
        return Ok(());
    }
    match command {
        Command::MigratePersistentClaims {} => {
            ensure!(actor == w.admin, "workspace owner required");
            w.persistent_claims = true;
        }
        Command::MigrateConsumers {} => {
            crate::consumer_migration::migrate(w, actor, position, recorded_at)?;
        }
        Command::MigrateConsumerDrops {} => {
            crate::consumer_drops::apply(w, actor, recorded_at)?;
        }
        Command::MigrateTestRules {} => {
            crate::test_rules::migrate(w, actor, position, recorded_at)?;
        }
        Command::MigrateBody {} => {
            crate::body_migration::migrate(w, actor, position, recorded_at)?;
        }
        Command::MigrateNoteParents {} => {
            crate::note_parents::migrate(w, actor, position, recorded_at)?;
        }
        Command::MigrateNotes {} => {
            ensure!(actor == w.admin, "workspace owner required");
            crate::objects::migrate_notes(w, actor, position, recorded_at)?;
        }
        Command::MigrateTasks {} => {
            ensure!(actor == w.admin, "workspace owner required");
            crate::tasks::migrate(w, actor, position, recorded_at)?;
        }
        Command::MigrateSchema {} => {
            ensure!(actor == w.admin, "workspace owner required");
            crate::objects::migrate_schema(w, actor, position, recorded_at)?;
        }
        Command::MigrateTaskClaims { .. } => apply_migrate_task_claims(w, actor)?,
        Command::HeartbeatTask { .. } => anyhow::bail!("unknown Task"),
        Command::MigrateDocuments {} => {
            ensure!(actor == w.admin, "workspace owner required");
            crate::objects::migrate_documents(w, actor, position, recorded_at)?;
        }
        Command::SetTreeSchema(crate::commands::SetTreeSchemaOp { .. }) => {
            apply_set_tree_schema(w, actor, command, position, recorded_at)?
        }
        Command::AssessSignal { .. } => apply_assess_signal(w, actor, command, position)?,
        Command::SetPriority { .. } => apply_set_priority(w, command)?,
        Command::SaveObject(crate::commands::SaveObjectOp { .. }) => {
            crate::objects::save(w, actor, command, position, recorded_at)?;
        }
        Command::ArchiveObject { .. } => {
            crate::objects::archive(w, actor, command, position, recorded_at)?;
        }
        Command::SetRemoteLinks { .. } => {
            apply_set_remote_links(w, actor, command, position, recorded_at)?
        }
        Command::RestoreObject { .. } => {
            crate::objects::restore(w, actor, command, position, recorded_at)?;
        }
        Command::RemoveWorkspaceReferences { .. } => {
            unreachable!("applied before the member check")
        }
        Command::SetAcTests(crate::commands::SetAcTestsOp {
            target,
            tests,
            revision,
            reason,
        }) => crate::ac_tests::set(
            w,
            actor,
            target,
            tests,
            *revision,
            reason,
            position,
            recorded_at,
        )?,
        Command::DecideNote(crate::commands::DecideNoteOp {
            target,
            revision,
            outcome,
            reason,
            links,
            duplicate_of,
            question,
            recipient,
        }) => {
            let decision = crate::inbox::Decision {
                target,
                revision: *revision,
                outcome,
                reason,
                links,
                duplicate_of: duplicate_of.as_deref(),
                question: question.as_deref(),
                recipient: recipient.as_deref(),
            };
            crate::inbox::decide(w, actor, &decision, position, recorded_at)?;
        }
        Command::AcceptCommit {
            target,
            revision,
            commit,
        } => crate::acceptance::accept(
            w,
            actor,
            target,
            *revision,
            commit.as_deref(),
            position,
            recorded_at,
        )?,
        Command::RedactObject {
            target,
            fragment,
            reason,
        } => crate::redact::redact(w, actor, target, fragment, reason, position, recorded_at)?,
        Command::SetNoteStatus(crate::commands::SetNoteStatusOp { .. }) => {
            apply_set_note_status(w, actor, command, position, recorded_at)?
        }
        Command::DeleteWorkspace { reason } => {
            ensure!(actor == w.admin, "workspace owner deletes the workspace");
            nonempty(reason)?;
            // Without members, every read and every later command of the workspace is refused.
            w.members.clear();
            w.deleted_at = Some(position);
        }
        Command::RenameWorkspace(crate::commands::RenameWorkspaceOp { .. }) => {
            apply_rename_workspace(w, actor, command)?
        }
        Command::ReorderTree {
            kind,
            parent,
            expected,
            ids,
        } => {
            crate::tree_order::reorder(w, actor, kind, parent.as_deref(), expected, ids)?;
        }
        Command::Member { .. } => apply_member(w, actor, command, position)?,
        Command::EnrollService { .. } => apply_enroll_service(w, actor, command)?,
        Command::RemoveMember { id, reason } => {
            ensure!(actor == w.admin, "workspace owner required");
            nonempty(reason)?;
            ensure!(*id != w.admin, "workspace owner cannot be removed");
            ensure!(w.members.remove(id).is_some(), "unknown member");
        }
        Command::Consumer(crate::commands::ConsumerOp { .. }) => apply_consumer(w, command)?,
        Command::EditConsumer(crate::commands::EditConsumerOp { .. }) => {
            apply_edit_consumer(w, actor, command)?
        }
        Command::Domain(crate::commands::DomainOp { .. }) => apply_domain(w, actor, command)?,
        Command::Capability(crate::commands::CapabilityOp { .. }) => {
            apply_capability(w, actor, command)?
        }
        Command::SetDomainArchived(crate::commands::SetDomainArchivedOp { .. }) => {
            apply_set_domain_archived(w, actor, command)?
        }
        Command::ConfirmTree(crate::commands::ConfirmTreeOp { .. }) => {
            apply_confirm_tree(w, actor)?
        }
        Command::EditDomain(crate::commands::EditDomainOp { .. }) => {
            apply_edit_domain(w, actor, command)?
        }
        Command::EditCapability(crate::commands::EditCapabilityOp { .. }) => {
            apply_edit_capability(w, actor, command)?
        }
        Command::WithdrawDecision(crate::commands::WithdrawDecisionOp { .. }) => {
            apply_withdraw_decision(w, actor, command)?
        }
        Command::Project(crate::commands::ProjectOp { id, repository }) => {
            identifier(id)?;
            nonempty(repository)?;
            ensure!(!w.projects.contains_key(id), "project exists");
            w.projects.insert(id.clone(), repository.clone());
        }
        Command::Revise(crate::commands::ReviseOp { .. }) => {
            apply_revise(w, actor, command, position)?
        }
        Command::LinkSource(crate::commands::LinkSourceOp { .. }) => {
            apply_link_source(w, actor, command, recorded_at)?
        }
        Command::WithdrawSource(crate::commands::WithdrawSourceOp { .. }) => {
            apply_withdraw_source(w, actor, command)?
        }
        Command::Confirm { .. } => apply_confirm(w, actor, command)?,
        Command::Describe(crate::commands::DescribeOp { .. }) => apply_describe(w, command)?,
        Command::Note { .. } => apply_note(w, actor, command, recorded_at)?,
        Command::WithdrawNote { .. } => apply_withdraw_note(w, actor, command)?,
        Command::Attach(crate::commands::AttachOp { .. }) => {
            apply_attach(w, actor, command, recorded_at)?
        }
        Command::Detach { .. } => apply_detach(w, command)?,
        Command::Comment(crate::commands::CommentOp { .. }) => {
            apply_comment(w, actor, command, recorded_at)?
        }
        Command::Assess { .. } => apply_assess(w, command)?,
        Command::Workspace { .. } => unreachable!(),
        _ => checks::apply(w, actor, command, position, recorded_at)?,
    }
    Ok(())
}
