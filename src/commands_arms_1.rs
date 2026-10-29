//! Arms of `apply_inner` in commands.rs, one function for each operation.
use super::*;

/// Apply `migrate_task_claims`.
pub(super) fn apply_migrate_task_claims(w: &mut Workspace, actor: &str) -> Result<()> {
    ensure!(actor == w.admin, "workspace owner required");
    if !w.claims_migrated {
        let now = w.claim_time_ms.context("server claim time is missing")?;
        for task in w.task_process.values_mut() {
            if task.assignee.is_some() && task.done.is_none() && task.retired.is_none() {
                task.claim = Some(crate::model::TaskClaim {
                    heartbeat_at_ms: now,
                    deadline_ms: now + crate::tasks::CLAIM_DURATION_MS,
                });
            }
        }
    }
    w.claims_migrated = true;
    Ok(())
}

/// Apply `set_tree_schema`.
pub(super) fn apply_set_tree_schema(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    let Command::SetTreeSchema(crate::commands::SetTreeSchemaOp {
        revision,
        types,
        put_types,
        remove_types,
        priorities,
        status_colors,
        statuses,
        test_results,
        status_required,
        reason,
    }) = command
    else {
        unreachable!()
    };
    let types = crate::schema_nodes::requested_types(
        w,
        types.as_ref(),
        put_types.as_ref(),
        remove_types.as_ref(),
    )?;
    crate::objects::set_schema(
        w,
        actor,
        *revision,
        &types,
        priorities.as_ref(),
        status_colors.as_ref(),
        statuses.as_ref(),
        test_results.as_ref(),
        *status_required,
        reason,
        position,
        recorded_at,
    )?;
    Ok(())
}

/// Apply `assess_signal`.
pub(super) fn apply_assess_signal(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
) -> Result<()> {
    let Command::AssessSignal {
        target,
        ready,
        reason,
    } = command
    else {
        unreachable!()
    };
    nonempty(reason)?;
    let key = crate::objects::resolve_address(w, target).to_owned();
    let kind = w
        .objects
        .get(&key)
        .filter(|o| o.current().present)
        .map(|o| o.kind.clone())
        .context("unknown object")?;
    ensure!(
        crate::objects::input_type(w, &kind),
        "only a signal of an inbox type takes a readiness mark"
    );
    w.signal_assessments.insert(
        key,
        crate::model::SignalAssessment {
            ready: *ready,
            reason: reason.clone(),
            actor: actor.into(),
            position,
        },
    );
    Ok(())
}

/// Apply `set_priority`.
pub(super) fn apply_set_priority(w: &mut Workspace, command: &Command) -> Result<()> {
    let Command::SetPriority { target, priority } = command else {
        unreachable!()
    };
    let key = crate::objects::resolve_address(w, target).to_owned();
    ensure!(
        w.objects.get(&key).is_some_and(|o| o.current().present),
        "unknown object"
    );
    let values = crate::objects::priority_values(w);
    ensure!(
        values.contains(priority),
        "priority is one of {}",
        values.join(", ")
    );
    w.priorities.insert(key, priority.clone());
    Ok(())
}

/// Apply `set_remote_links`.
pub(super) fn apply_set_remote_links(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    let Command::SetRemoteLinks {
        target,
        revision,
        links,
        reason,
    } = command
    else {
        unreachable!()
    };
    nonempty(reason)?;
    ensure!(
        crate::objects::may_edit_tree(w, actor),
        "workspace member required"
    );
    let key = crate::objects::resolve_address(w, target).to_owned();
    ensure!(
        w.objects.get(&key).is_some_and(|o| o.current().present),
        "unknown object"
    );
    crate::remote_links::validate(links)?;
    // A pin is part of the contract that the criteria below were tested against, so a
    // new, changed or removed pin resets them. An ordering link without a pin does not.
    let pinned = |links: &[crate::remote_links::RemoteLink]| -> BTreeSet<_> {
        links
            .iter()
            .filter(|l| l.revision.is_some())
            .cloned()
            .map(|l| (l.workspace, l.target, l.relation, l.revision))
            .collect()
    };
    let current = w.remote_links.get(&key).cloned().unwrap_or_default();
    ensure!(
        current.revision == *revision,
        "stale remote links revision; read the object again"
    );
    let repinned = pinned(&current.links) != pinned(links);
    w.remote_links.insert(
        key.clone(),
        crate::remote_links::RemoteLinks {
            revision: current.revision + 1,
            links: links.clone(),
            position,
        },
    );
    if repinned {
        crate::objects::reset_criteria(w, &key, recorded_at)?;
    }
    Ok(())
}

/// Apply `set_note_status`.
pub(super) fn apply_set_note_status(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    let Command::SetNoteStatus(crate::commands::SetNoteStatusOp {
        target,
        revision,
        status,
        reason,
    }) = command
    else {
        unreachable!()
    };
    let key = crate::objects::resolve_address(w, target);
    let note = w
        .objects
        .get(key)
        .ok_or_else(|| anyhow::anyhow!("unknown Note"))?;
    ensure!(
        crate::objects::input_type(w, &note.kind),
        "set_note_status requires an inbox type"
    );
    ensure!(
        ["unprocessed", "processed"].contains(&status.as_str()),
        "unknown Note status"
    );
    let r = note.current();
    let mut data = r.data.clone();
    data["status"] = serde_json::json!(status);
    let command = Command::SaveObject(crate::commands::SaveObjectOp {
        question_checked: None,
        kind: note.kind.clone(),
        id: note.id.clone(),
        revision: *revision,
        parent: r.parent.clone(),
        data,
        links: r.links.clone(),
        reason: reason.clone(),
    });
    crate::objects::save(w, actor, &command, position, recorded_at)?;
    Ok(())
}
