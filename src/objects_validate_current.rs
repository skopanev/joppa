//! Part of `objects`: `validate_current` and the items after it, moved from objects.rs unchanged.
use super::*;

pub fn validate_current(w: &Workspace, cmd: &Command) -> Result<()> {
    validate_title_length(w, cmd)?;
    if let Command::SaveObject(crate::commands::SaveObjectOp { kind, id, data, .. }) = cmd {
        let stored = w
            .objects
            .get(resolve_address(w, &address(kind, id)))
            .map(|o| &o.current().data);
        crate::field_limit::check(data, stored)?;
    }
    task_links_same_requirement(w, cmd)?;
    if let Command::SaveObject(crate::commands::SaveObjectOp {
        kind,
        id,
        parent,
        links,
        ..
    }) = cmd
    {
        validate_wait_cycle(w, &address(kind, id), parent.as_deref(), links)?;
    }
    match cmd {
        Command::MigrateNotes {}
        | Command::MigrateTasks {}
        | Command::MigrateSchema {}
        | Command::MigrateDocuments {}
        | Command::MigrateTaskClaims {}
        | Command::MigrateConsumers {}
        | Command::MigrateConsumerDrops {}
        | Command::MigrateTestRules {}
        | Command::MigrateNoteParents {} => {
            bail!("internal migration operation")
        }
        Command::Intake(crate::commands::IntakeOp { .. })
        | Command::Split(crate::commands::SplitOp { .. })
        | Command::Questions(crate::commands::QuestionsOp { .. })
        | Command::Sort(crate::commands::SortOp { .. })
        | Command::Answer(crate::commands::AnswerOp { .. })
        | Command::Route(crate::commands::RouteOp { .. }) => {
            bail!("use save_object kind=note; incoming source types are removed")
        }
        Command::SaveObject(crate::commands::SaveObjectOp {
            kind, id, revision, ..
        }) if !w.documents_migrated => {
            ensure!(
                !["intake", "point"].contains(&kind.as_str()),
                "use kind=note"
            );
            ensure!(
                kind != "note" || *revision != 0 || !id.starts_with("source-"),
                "Note migration identifiers are reserved"
            );
        }
        Command::SetTreeSchema(crate::commands::SetTreeSchemaOp {
            types, put_types, ..
        }) if !w.documents_migrated => ensure!(
            [types, put_types]
                .into_iter()
                .flatten()
                .all(|t| !t.contains_key("intake") && !t.contains_key("point")),
            "incoming source types are removed; use Note"
        ),
        Command::CompleteTask(crate::commands::CompleteTaskOp {
            req, id, commit, ..
        }) => {
            ensure!(
                !w.test_rules_migrated || commit.is_some(),
                "missing field `commit`: complete_task needs the git sha where the Task is ready"
            );
            ensure!(
                work_ready(w, &crate::tasks::address(req, id)),
                "Task has an unmet dependency or a changed dependency revision"
            )
        }
        Command::ReviewTask(crate::commands::ReviewTaskOp {
            req,
            revision,
            id,
            ac,
            ..
        }) => {
            if req.is_empty() {
                return Ok(());
            }
            let r = w
                .requirements
                .get(req)
                .and_then(|q| q.revisions.last())
                .context("unknown REQ")?;
            ensure!(r.number == *revision, "stale REQ revision");
            let task = r.tasks.get(id).context("unknown Task")?;
            ensure!(
                task.done.is_none() || task.acs == vec![ac.clone()],
                "completed Task cannot move to another AC"
            );
            ensure!(task.retired.is_none(), "retired Task cannot be reviewed");
        }
        Command::Accept(crate::commands::AcceptOp { req, .. }) => {
            let r = w
                .requirements
                .get(req)
                .and_then(|q| q.revisions.last())
                .context("unknown REQ")?;
            ensure!(
                requirement_summary(w, req, r)["ready_for_acceptance"] == true,
                "REQ has unverified ACs or an unmet dependency"
            );
        }
        Command::SetDomainArchived(crate::commands::SetDomainArchivedOp { id, .. }) => ensure!(
            !w.archived_objects.contains(&address("domain", id)),
            "restore the archived object before editing it"
        ),
        _ => {}
    }
    Ok(())
}

pub(crate) fn criterion_record_context<'a>(
    w: &'a Workspace,
    key: &str,
) -> Option<(&'a str, &'a Revision, &'a Ac)> {
    let key = resolve_address(w, key);
    if let Some(context) = criterion_context(w, key) {
        return Some(context);
    }
    let obj = w.objects.get(key)?;
    if obj.current().present {
        return None;
    }
    for (req, q) in &w.requirements {
        if let Some(ac) = q
            .revisions
            .iter()
            .rev()
            .flat_map(|r| r.acs.iter())
            .find(|ac| criterion_address(&ac.id) == key)
        {
            return Some((req, q.revisions.last()?, ac));
        }
    }
    None
}

pub fn annotation_address(w: &Workspace, key: &str) -> String {
    let key = resolve_address(w, key);
    if let Some((req, _, ac)) = criterion_record_context(w, key)
        && !ac.id.contains(':')
        && !req.contains(':')
    {
        return format!("ac:{req}/{}", ac.id);
    }
    key.into()
}

/// Resolve the configured verification role against the journal evidence adapter.
pub fn verification_context<'a>(w: &'a Workspace, key: &str) -> Option<(&'a str, &'a Revision)> {
    let mut current = Some(resolve_address(w, key));
    let mut seen = BTreeSet::new();
    while let Some(key) = current {
        if !seen.insert(key) {
            return None;
        }
        let o = w.objects.get(key)?;
        let role = schema(w).types.get(&o.kind).map(|t| t.role.clone());
        if role.as_deref() == Some("requirement")
            && !w.document_overrides.contains(key)
            && let Some(req) = requirement_key(w, key)
            && let Some(r) = w.requirements.get(req).and_then(|q| q.revisions.last())
        {
            return Some((req, r));
        }
        current = o.current().parent.as_deref();
    }
    None
}

pub fn criterion_context<'a>(
    w: &'a Workspace,
    key: &str,
) -> Option<(&'a str, &'a Revision, &'a Ac)> {
    let key = resolve_address(w, key);
    let o = w.objects.get(key)?;
    if w.document_overrides.contains(key) || schema(w).types.get(&o.kind)?.role != "ac" {
        return None;
    }
    let (req, revision) = verification_context(w, o.current().parent.as_deref()?)?;
    let ac = revision
        .acs
        .iter()
        .find(|ac| criterion_address(&ac.id) == key)?;
    Some((req, revision, ac))
}

/// Read the assessment from the current configured verification context.
pub fn assessment(w: &Workspace, key: &str) -> Option<Value> {
    let (req, r, ac) = criterion_context(w, key)?;
    let state = match ac.assessment.as_ref() {
        None => "unassessed",
        Some(a) if a.ready => "ready",
        Some(_) => "waiting",
    };
    let mut out = json!({"state":state,"req":req,"revision":r.number,"criterion":ac.id,
        "reason":ac.assessment.as_ref().map(|a| &a.reason),
        "recipient":ac.assessment.as_ref().and_then(|a| a.owner.as_ref()),
        "decision":ac.assessment.as_ref().and_then(|a| a.decision.as_ref())});
    if state == "unassessed" {
        // The next step: the command that sets readiness, without the assessment itself.
        out["next"] = json!({"op":"assess","req":req,"revision":r.number,"ac":ac.id});
    }
    Some(out)
}
