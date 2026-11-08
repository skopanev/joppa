//! Part of `objects`: `validate_current` and the items after it, moved from objects.rs unchanged.
use super::*;

pub fn validate_current(w: &Workspace, cmd: &Command) -> Result<()> {
    validate_title_length(w, cmd)?;
    if let Command::SaveObject(crate::commands::SaveObjectOp { kind, id, data, .. }) = cmd {
        let stored = w
            .objects
            .get(resolve_address(w, &address(kind, id)))
            .map(|o| &o.current().data);
        let limit = schema(w).types.get(kind).and_then(|t| t.field_max_length);
        crate::field_limit::check(data, stored, crate::field_limit::limit(limit))?;
    }
    if let Command::SaveObject(crate::commands::SaveObjectOp {
        kind,
        id,
        parent,
        links,
        ..
    }) = cmd
    {
        let links = links.clone().unwrap_or_default();
        validate_wait_cycle(w, &address(kind, id), parent.as_deref(), &links)?;
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
        | Command::MigrateNoteParents {}
        | Command::MigrateBody {}
        | Command::MigratePurpose {}
        | Command::MigrateTestNodes {}
        | Command::MigrateGenericNodes {}
        | Command::MigrateDecisionRequired {} => {
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
