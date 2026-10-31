//! Part of `objects`: `saved` and the items after it, moved from objects.rs unchanged.
use super::*;

pub(crate) fn saved(
    w: &Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
    accept_unchanged: bool,
) -> Result<Option<Workspace>> {
    let Command::SaveObject(crate::commands::SaveObjectOp {
        id,
        kind,
        revision,
        parent,
        data,
        links,
        reason,
        ..
    }) = command
    else {
        bail!("save_object required")
    };
    let parent = &parent
        .as_deref()
        .map(|key| resolve_address(w, key).to_owned());
    commands::nonempty(reason)?;
    ensure!(
        !id.is_empty()
            && id.len() <= 200
            && !id.contains(':')
            && !id.chars().any(char::is_whitespace),
        "invalid object id"
    );
    commands::identifier(kind)?;
    let key = address(kind, id);
    ensure!(
        !w.documents_migrated || !w.object_aliases.contains_key(&key),
        "object address is a historical alias"
    );
    let old = w.objects.get(&key).cloned();
    ensure!(
        old.as_ref().map_or(0, |o| o.current().revision) == *revision,
        "stale object revision; read the object again"
    );
    if let Some(old) = &old {
        ensure!(
            old.current().present,
            "object is absent from the current tree"
        );
    }
    ensure!(data.is_object(), "data must be an object");
    ensure!(
        data.get("verification").is_none(),
        "verification comes from test reports"
    );
    crate::body_migration::check_save(w, data)?;
    if w.documents_migrated {
        for field in data.as_object().unwrap().keys() {
            ensure!(!process_field(field), "use process commands for {field}");
        }
    }
    if input_type(w, kind) {
        ensure!(
            data.get("status")
                .is_none_or(|v| matches!(v.as_str(), Some("unprocessed" | "processed"))),
            "Note status must be unprocessed or processed"
        );
    }
    commands::nonempty(&string(data, "title")?)?;
    validate_required_fields(
        schema(w).types.get(kind).context("unknown object type")?,
        data,
    )?;
    if w.test_rules_migrated {
        crate::ac_tests::validate_field(w, kind, data)?;
    }
    crate::note_parents::check_save(w, kind, parent.as_deref())?;
    parent_valid(&schema(w).types, w, kind, parent.as_deref(), Some(&key))?;
    parent_cycle(w, &key, parent.as_deref())?;
    if w.schema_migrated {
        reference_targets(w, kind, data)?;
    }
    let kept = old
        .as_ref()
        .map(|o| o.current().links.clone())
        .unwrap_or_default();
    validate_links(w, &key, links, &kept)?;
    let unchanged = old
        .as_ref()
        .is_some_and(|o| o.current().data == *data && o.current().parent == *parent);
    let same = |saved: &Value| {
        accept_unchanged
            && old.as_ref().is_some_and(|o| {
                let r = o.current();
                r.data == *saved && r.parent == *parent && r.links == *links
            })
    };
    let mut candidate = w.clone();
    let generic_task = schema(w).types[kind].role == "task" && w.tasks_migrated;
    let task_data = if generic_task {
        Some(crate::tasks::save(
            &mut candidate,
            actor,
            &key,
            data,
            parent.as_deref(),
            position,
            same,
        )?)
    } else {
        None
    };
    // A change of only the test list is no change of the specification: the document gets a
    // revision, and the specification keeps its revision.
    let tests_only = w.test_rules_migrated
        && old.as_ref().is_some_and(|o| {
            let without = |d: &Value| {
                let mut d = d.clone();
                if let Some(map) = d.as_object_mut() {
                    map.remove("tests");
                }
                d
            };
            without(&o.current().data) == without(data) && o.current().parent == *parent
        });
    if (!unchanged && !tests_only || schema(w).types[kind].role == "requirement") && !generic_task {
        typed_save(w, &mut candidate, actor, command, position, at, &key, &old)?;
    }
    let schema_revision = schema(w).revision;
    let projected = candidate
        .objects
        .get(&key)
        .map(|o| o.current().data.clone())
        .unwrap_or_else(|| data.clone());
    // A custom parent changes grouping. The legacy semantic ancestor remains explicit.
    let mut object = candidate
        .objects
        .get(&key)
        .cloned()
        .unwrap_or_else(|| Object {
            id: id.clone(),
            kind: kind.clone(),
            revisions: vec![],
            parent_override: false,
        });
    object.revisions = old
        .as_ref()
        .map(|o| o.revisions.clone())
        .unwrap_or_default();
    let mut saved_data = if let Some(data) = task_data {
        data
    } else if !candidate.document_overrides.contains(&key)
        && (w.documents_migrated
            && matches!(schema(w).types[kind].role.as_str(), "requirement" | "ac")
            || !w.documents_migrated && legacy_defaults().contains_key(kind) && kind != "note")
    {
        if w.schema_migrated {
            let mut normalized = projected;
            normalized.as_object_mut().unwrap().retain(|field, _| {
                data.get(field).is_some()
                    || document_adapter_fields(w, kind)
                        .is_some_and(|fields| fields.contains(&field.as_str()))
            });
            for (field, value) in data.as_object().unwrap() {
                normalized[field] = value.clone();
            }
            if !w.documents_migrated {
                for field in schema_migration_defaults()
                    .get(kind)
                    .into_iter()
                    .flat_map(|t| t.references.keys())
                {
                    if !schema(w).types[kind].references.contains_key(field)
                        && data.get(field).is_none()
                    {
                        normalized.as_object_mut().unwrap().remove(field);
                    }
                }
            }
            normalized
        } else {
            projected
        }
    } else {
        data.clone()
    };
    // A new signal is unprocessed until someone decides on it.
    if input_type(w, kind) && saved_data["status"].is_null() {
        saved_data["status"] = json!("unprocessed");
    }
    if input_type(w, kind)
        && let Some(old) = &old
    {
        let source = |value: &Value| {
            let mut value = value.clone();
            if let Some(fields) = value.as_object_mut() {
                fields.remove("status");
                fields.remove("owner");
                // A decision is a record about the signal, not a change of the signal.
                fields.remove("decision");
            }
            value
        };
        if saved_data["status"] == "processed" && source(&old.current().data) != source(&saved_data)
        {
            saved_data["status"] = json!("unprocessed");
        }
    }
    if w.body_migrated {
        crate::body_migration::to_body(&mut saved_data);
    }
    if same(&saved_data) {
        return Ok(None);
    }
    object.parent_override = document_rows(&candidate)
        .get(&key)
        .is_none_or(|(_, p, _)| p != parent);
    object.revisions.push(ObjectRevision {
        revision: revision + 1,
        schema_revision,
        parent: parent.clone(),
        data: saved_data,
        links: links.clone(),
        actor: actor.into(),
        at: at.into(),
        position,
        reason: reason.clone(),
        present: true,
        contract: None,
    });
    candidate.objects.insert(key.clone(), object);
    validate_links(&candidate, &key, links, &kept)?;
    Ok(Some(candidate))
}
