//! Part of `objects`: `stored_reference_targets` and the items after it, moved from objects.rs unchanged.
use super::*;

/// The reference targets of a stored document for a read. A document from before a
/// required reference existed still gives its other targets.
pub fn stored_reference_targets(
    w: &Workspace,
    kind: &str,
    data: &Value,
) -> Result<Vec<(String, String, bool)>> {
    targets_of(w, kind, data, false)
}

pub(crate) fn targets_of(
    w: &Workspace,
    kind: &str,
    data: &Value,
    enforce_required: bool,
) -> Result<Vec<(String, String, bool)>> {
    let t = schema(w)
        .types
        .get(kind)
        .cloned()
        .context("unknown object type")?;
    let mut targets = vec![];
    for (field, rule) in &t.references {
        // The explicit value all names every object of the target type.
        if rule.allow_all && data.get(field).and_then(Value::as_str) == Some("all") {
            continue;
        }
        let values: Vec<&str> = match data.get(field) {
            None | Some(Value::Null) => vec![],
            Some(Value::Array(values)) if rule.multiple => values
                .iter()
                .map(|v| v.as_str().context("reference IDs must be strings"))
                .collect::<Result<_>>()?,
            Some(Value::String(value)) if !rule.multiple => vec![value],
            _ => bail!("reference {field} has invalid cardinality"),
        };
        ensure!(
            !enforce_required || !rule.required || !values.is_empty(),
            "reference {field} is required"
        );
        let mut seen = BTreeSet::new();
        for value in values {
            let target = if value.contains(':') {
                value.into()
            } else {
                ensure!(
                    rule.target != "*",
                    "a wildcard reference needs a full object address"
                );
                address(&rule.target, value)
            };
            let target = resolve_address(w, &target).to_owned();
            let object = w
                .objects
                .get(&target)
                .filter(|o| o.current().present)
                .context(format!("unknown reference target: {target}"))?;
            ensure!(
                rule.target == "*" || rule.target == object.kind,
                "reference {field} targets the wrong object type"
            );
            ensure!(
                seen.insert(target.clone()),
                "duplicate reference in {field}"
            );
            targets.push((field.clone(), target, rule.verification_subject));
        }
    }
    Ok(targets)
}

pub fn referenced(w: &Workspace, key: &str, target: &str) -> bool {
    let target = resolve_address(w, target);
    let mut current = Some(key);
    let mut seen = BTreeSet::new();
    while let Some(key) = current {
        let key = resolve_address(w, key);
        if !seen.insert(key) {
            return false;
        }
        let Some(o) = w.objects.get(key) else {
            return false;
        };
        if stored_reference_targets(w, &o.kind, &o.current().data)
            .is_ok_and(|refs| refs.iter().any(|(_, id, _)| id == target))
        {
            return true;
        }
        current = o.current().parent.as_deref();
    }
    false
}

pub(crate) fn address(kind: &str, id: &str) -> String {
    format!("{kind}:{id}")
}

pub fn resolve_address<'a>(w: &'a Workspace, key: &'a str) -> &'a str {
    w.object_aliases.get(key).map(String::as_str).unwrap_or(key)
}

pub fn original_identity<'a>(w: &'a Workspace, kind: &'a str, id: &'a str) -> (&'a str, &'a str) {
    let key = address(kind, id);
    w.object_aliases
        .iter()
        .find(|(_, target)| *target == &key)
        .and_then(|(source, _)| source.split_once(':'))
        .unwrap_or((kind, id))
}

pub fn migrate_notes(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.notes_migrated {
        return Ok(());
    }
    let mut candidate = w.clone();
    let previous_schema = schema(w);
    let reason = NOTES_SCHEMA_REASON;
    let sources: Vec<_> = w
        .objects
        .iter()
        .filter(|(_, o)| o.current().present && ["intake", "point"].contains(&o.kind.as_str()))
        .map(|(key, o)| (key.clone(), o.clone()))
        .collect();
    for (key, _) in &sources {
        let target = format!(
            "note:source-{}",
            crate::digest_hex::sha256_hex(key.as_bytes())
        );
        ensure!(
            !w.objects.contains_key(&target),
            "Note migration address collision"
        );
        candidate.object_aliases.insert(key.clone(), target);
    }
    let mut types = previous_schema.types;
    types.remove("intake");
    types.remove("point");
    for t in types.values_mut() {
        for parent in &mut t.parents {
            if ["intake", "point"].contains(&parent.as_str()) {
                *parent = "note".into();
            }
        }
        let mut seen = BTreeSet::new();
        t.parents.retain(|parent| seen.insert(parent.clone()));
    }
    let schema_revision = previous_schema.revision + 1;
    candidate.tree_schema.push(SchemaRevision {
        revision: schema_revision,
        types,
        priorities: previous_schema.priorities.clone(),
        status_colors: previous_schema.status_colors.clone(),
        statuses: previous_schema.statuses.clone(),
        test_results: previous_schema.test_results.clone(),
        status_required: previous_schema.status_required,
        actor: actor.into(),
        at: at.into(),
        position,
        reason: reason.into(),
    });
    for (key, old) in sources {
        let target = candidate.object_aliases[&key].clone();
        let mut current = old.current().clone();
        current.revision += 1;
        current.schema_revision = schema_revision;
        current.actor = actor.into();
        current.at = at.into();
        current.position = position;
        current.reason = reason.into();
        current.parent = current
            .parent
            .as_deref()
            .map(|parent| resolve_address(&candidate, parent).to_owned());
        if old.kind == "point" {
            current.data["body"] = current.data["title"].clone();
            let source = old.id.split_once('/').context("invalid source point")?.0;
            if let Some(intake) = w.intake.get(source) {
                current.data["author"] = json!(intake.author);
            }
            let kind = current.data["kind"].as_str().unwrap_or("");
            current.data["review_policy"] = json!(w.questions.get(kind));
            if let Some(req) = current.data["requirement"].as_str() {
                let link = ObjectLink {
                    target: requirement_address(req),
                    relation: "related".into(),
                    revision: None,
                    blocking: false,
                };
                if !current
                    .links
                    .iter()
                    .any(|l| l.target == link.target && l.relation == link.relation)
                {
                    current.links.push(link);
                }
            }
        } else {
            current.data["review_policy"] = json!(w.questions);
        }
        let mut note = old.clone();
        note.kind = "note".into();
        note.id = target.strip_prefix("note:").unwrap().into();
        note.parent_override = true;
        note.revisions.push(current);
        candidate.objects.insert(target.clone(), note);
        let mut retired = old.current().clone();
        retired.revision += 1;
        retired.present = false;
        retired.actor = actor.into();
        retired.at = at.into();
        retired.position = position;
        retired.reason = reason.into();
        candidate
            .objects
            .get_mut(&key)
            .unwrap()
            .revisions
            .push(retired);
        if let Some(notes) = candidate.notes.remove(&key) {
            candidate.notes.insert(target, notes);
        }
    }
    candidate.intake.clear();
    candidate.questions.clear();
    candidate.notes_migrated = true;
    validate_tree(&candidate)?;
    *w = candidate;
    Ok(())
}
