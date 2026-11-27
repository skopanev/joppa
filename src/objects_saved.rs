//! Part of `objects`: the new document revision of save_object.
use super::*;

/// The workspace after save_object, or None when the save changes nothing.
pub(crate) fn saved(
    w: &Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
    accept_unchanged: bool,
) -> Result<Option<Workspace>> {
    crate::probe::count(&crate::probe::SAVE_COPY);
    let mut candidate = w.clone();
    let changed = save_into(
        &mut candidate,
        actor,
        command,
        position,
        at,
        accept_unchanged,
    )?;
    Ok(changed.then_some(candidate))
}

/// Write the new revision of save_object into `w`; false when it changes nothing. A refused or
/// unchanged save can leave work in `w`, so a caller that keeps `w` passes a copy. After its
/// first write the save reads only the schema, the flags and the members, which it keeps.
pub(crate) fn save_into(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
    accept_unchanged: bool,
) -> Result<bool> {
    // A confirmation of the parent hash is no data field (parent_hash.rs).
    let (stripped, sent) = without_parent_hash(w, command, position)?;
    let command: &Command = &stripped;
    let Command::SaveObject(crate::commands::SaveObjectOp {
        id,
        kind,
        revision,
        parent,
        data,
        links,
        relations,
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
    let old = w.objects.get(&key).map(|o| Object::clone(o));
    let current = old.as_ref().map_or(0, |o| o.current().revision);
    ensure!(
        current == *revision,
        "stale object revision on {key}: sent {revision}, current is {current}"
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
    crate::purpose_migration::check_save(w, data)?;
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
    // A priority is one value of the priority list of the tree schema.
    if let Some(priority) = data.get("priority").filter(|p| !p.is_null()) {
        let values = priority_values(w);
        ensure!(
            priority
                .as_str()
                .is_some_and(|p| values.iter().any(|v| v == p)),
            "priority is one of {}",
            values.join(", ")
        );
    }
    commands::nonempty(&string(data, "title")?)?;
    let types = schema(w).types;
    let t = types.get(kind).with_context(|| {
        let valid: Vec<&str> = types.keys().map(String::as_str).collect();
        format!(
            "unknown object type {kind}: valid types are {}",
            valid.join(", ")
        )
    })?;
    validate_required_fields(t, data)?;
    if w.test_rules_migrated && !w.test_nodes_migrated {
        crate::ac_tests::validate_field(w, kind, data)?;
    }
    crate::test_nodes::check_save(w, kind, &key, parent.as_deref(), data)?;
    crate::note_parents::check_save(w, kind, parent.as_deref())?;
    parent_valid(&schema(w).types, w, kind, parent.as_deref(), Some(&key))?;
    parent_cycle(w, &key, parent.as_deref())?;
    if let Some(sent) = &sent {
        check_confirmation(w, sent, parent.as_deref())?;
    }
    if w.schema_migrated {
        reference_targets(w, kind, data)?;
    }
    let kept = old.as_ref().map(|o| o.current().relations.clone());
    let kept = kept.unwrap_or_default();
    let live = (!w.replaying).then_some(&kept);
    let (relations, form) = crate::relations::of_save(relations.as_ref(), links.as_deref(), live);
    crate::relations::validate(w, &key, &relations, &kept)?;
    let unchanged = old
        .as_ref()
        .is_some_and(|o| o.current().data == *data && o.current().parent == *parent);
    let same = |saved: &Value| {
        accept_unchanged
            && old.as_ref().is_some_and(|o| {
                let r = o.current();
                r.data == *saved && r.parent == *parent && r.relations == relations
            })
    };
    // Before migrate_generic_nodes a save of a Task also writes its Task record.
    let generic_task = schema(w).types[kind].role == "task" && w.tasks_migrated && !w.generic_nodes;
    let task_data = if generic_task {
        Some(crate::tasks::save(
            w,
            actor,
            &key,
            data,
            parent.as_deref(),
            position,
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
        typed_save(w, actor, command, position, at, &key, &old)?;
    }
    let schema_revision = schema(w).revision;
    let projected = w
        .objects
        .get(&key)
        .map(|o| o.current().data.clone())
        .unwrap_or_else(|| data.clone());
    // A custom parent changes grouping. The legacy semantic ancestor remains explicit.
    let mut object = old.clone().unwrap_or_else(|| Object {
        id: id.clone(),
        kind: kind.clone(),
        revisions: vec![],
        parent_override: false,
    });
    object.revisions = old
        .as_ref()
        .map(|o| o.revisions.clone())
        .unwrap_or_default();
    let mut saved_data = saved_data(w, (kind, &key), data, projected, task_data);
    input_status(w, kind, old.as_ref(), &mut saved_data);
    if w.body_migrated {
        crate::body_migration::to_body(&mut saved_data);
    }
    let parent_hash = match commands::rule_applies(w, commands::PARENT_HASH, position) {
        true => stamp(
            w,
            kind,
            old.as_ref(),
            (parent.as_deref(), &saved_data),
            sent.is_some(),
        ),
        false => old.as_ref().and_then(|o| o.current().parent_hash.clone()),
    };
    let stamped = old
        .as_ref()
        .is_some_and(|o| o.current().parent_hash == parent_hash);
    if same(&saved_data) && stamped {
        return Ok(false);
    }
    object.parent_override = document_parent(w, &key).is_none_or(|p| &p != parent);
    object.revisions.push(ObjectRevision {
        revision: revision + 1,
        schema_revision,
        parent: parent.clone(),
        data: saved_data,
        relations: relations.clone(),
        older_links: form,
        actor: actor.into(),
        alias: w.alias.clone(),
        at: at.into(),
        position,
        reason: reason.clone(),
        present: true,
        contract: None,
        parent_hash,
    });
    w.objects.insert(key.clone(), std::sync::Arc::new(object));
    crate::relations::validate(w, &key, &relations, &kept)?;
    Ok(true)
}

#[cfg(test)]
#[path = "objects_saved_tests.rs"]
mod tests;
