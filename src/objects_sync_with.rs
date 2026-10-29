//! Part of `objects`: `sync_with` and the items after it, moved from objects.rs unchanged.
use super::*;

/// `sync` that can leave unchanged sibling criteria of a revised specification alone.
/// With `force_siblings`, every criterion of the specification gets a new revision.
/// `spec_reason` replaces the reason of the new specification revision.
pub(crate) fn sync_with(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
    force_siblings: bool,
    spec_reason: Option<&str>,
) {
    let rows = document_rows(w);
    let reason = serde_json::to_value(command)
        .ok()
        .and_then(|v| v["reason"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "Journal operation".into());
    let configured = schema(w);
    let schema_revision = configured.revision;
    let adapter_fields: BTreeMap<_, _> = configured
        .types
        .keys()
        .map(|kind| (kind.clone(), document_adapter_fields(w, kind)))
        .collect();
    let forced_req = match command {
        Command::Revise(crate::commands::ReviseOp { req, .. }) => Some(req.as_str()),
        _ => None,
    };
    for (key, (kind, parent, data)) in &rows {
        if w.task_process.contains_key(key)
            || w.document_overrides.contains(key)
            || w.archived_objects.contains(key)
        {
            continue;
        }
        let obj = w.objects.entry(key.clone()).or_insert_with(|| Object {
            kind: kind.clone(),
            id: key.split_once(':').unwrap().1.into(),
            revisions: vec![],
            parent_override: false,
        });
        if matches!(command,Command::Revise(crate::commands::ReviseOp {req,capability,..}) if key==&requirement_address(req) && capability.as_ref().is_some_and(|cap|obj.revisions.last().is_some_and(|r|r.parent.as_deref()!=Some(&address("capability",cap)))))
        {
            obj.parent_override = false;
        }
        // The migrated specification owns its parent through both sync passes.
        // Only an explicit legacy grouping change can replace that parent here.
        let preserve_document_parent = w.documents_migrated
            && !obj.revisions.is_empty()
            && configured
                .types
                .get(kind)
                .is_some_and(|t| t.role == "requirement")
            && !matches!(command, Command::Revise(crate::commands::ReviseOp { req, capability: Some(_), .. }) if key == &requirement_address(req));
        let parent = if obj.parent_override || preserve_document_parent {
            obj.revisions.last().and_then(|v| v.parent.clone())
        } else {
            parent.clone()
        };
        let forced = forced_req.is_some_and(|req| {
            key == &requirement_address(req)
                || (force_siblings
                    && w.requirements
                        .get(req)
                        .and_then(|q| q.revisions.last())
                        .is_some_and(|r| r.acs.iter().any(|ac| criterion_address(&ac.id) == *key)))
        });
        let mut data = data.clone();
        if w.schema_migrated
            && let Some(previous) = obj.revisions.last()
            && let Some(t) = w.tree_schema.last().and_then(|s| s.types.get(kind))
        {
            for (field, value) in previous.data.as_object().unwrap() {
                let legacy_reference_update = field == "consumers"
                    && matches!(command, Command::Revise(crate::commands::ReviseOp { req, consumers, .. }) if key == &requirement_address(req) && w.documents_migrated && !consumers.is_empty());
                if !legacy_reference_update
                    && adapter_fields
                        .get(kind)
                        .copied()
                        .flatten()
                        .is_none_or(|fields| !fields.contains(&field.as_str()))
                {
                    data[field] = value.clone();
                }
            }
            for field in t.references.keys() {
                let native_update = field == "consumers"
                    && matches!(command, Command::Revise(crate::commands::ReviseOp { req, consumers, .. }) if key == &requirement_address(req) && (!w.documents_migrated || !consumers.is_empty()));
                if native_update && !t.references[field].multiple {
                    data[field] = data[field]
                        .as_array()
                        .and_then(|values| values.first())
                        .cloned()
                        .unwrap_or(Value::Null);
                } else if !native_update && let Some(value) = previous.data.get(field) {
                    data[field] = value.clone();
                }
            }
        }
        if w.schema_migrated
            && !w.documents_migrated
            && let Some(previous) = obj.revisions.last()
            && let Some(t) = w.tree_schema.last().and_then(|s| s.types.get(kind))
        {
            for field in schema_migration_defaults()
                .get(kind)
                .into_iter()
                .flat_map(|t| t.references.keys())
            {
                if !t.references.contains_key(field) && previous.data.get(field).is_none() {
                    data.as_object_mut().unwrap().remove(field);
                }
            }
        }
        // Only a new criterion takes a consumer here; a stored one keeps its document.
        if obj.revisions.is_empty()
            && let Some(t) = configured.types.get(kind)
        {
            let requirement = parent.as_ref().and_then(|p| rows.get(p)).map(|(_, _, d)| d);
            crate::consumer_migration::fill(t, &mut data, requirement);
        }
        let changed = obj
            .revisions
            .last()
            .is_none_or(|v| v.data != data || v.parent != parent || !v.present)
            || forced;
        if changed {
            let links = obj
                .revisions
                .last()
                .map(|v| v.links.clone())
                .unwrap_or_default();
            let spec = forced_req.is_some_and(|req| key == &requirement_address(req));
            obj.revisions.push(ObjectRevision {
                revision: obj.revisions.last().map_or(1, |r| r.revision + 1),
                schema_revision,
                parent,
                data: data.clone(),
                links,
                actor: actor.into(),
                at: at.into(),
                position,
                reason: spec_reason
                    .filter(|_| spec)
                    .map_or_else(|| reason.clone(), str::to_owned),
                present: true,
                contract: None,
            });
        }
    }
    let roles = schema(w).types;
    for (key, obj) in &mut w.objects {
        if (legacy_defaults().contains_key(&obj.kind)
            || roles
                .get(&obj.kind)
                .is_some_and(|t| t.role == "ac" || t.role == "requirement"))
            && obj.kind != "note"
            && !w.task_process.contains_key(key)
            && !w.document_overrides.contains(key)
            && !rows.contains_key(key)
            && obj.current().present
        {
            let mut r = obj.current().clone();
            r.revision += 1;
            r.present = false;
            r.actor = actor.into();
            r.at = at.into();
            r.position = position;
            r.reason = reason.clone();
            obj.revisions.push(r);
        }
    }
}
