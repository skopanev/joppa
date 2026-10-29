//! Part of `objects`: `typed_save` and the items after it, moved from objects.rs unchanged.
use super::*;

/// Apply the typed command of a saved document, or keep the document as an override.
#[allow(clippy::too_many_arguments)]
pub(crate) fn typed_save(
    w: &Workspace,
    candidate: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
    key: &str,
    old: &Option<Object>,
) -> Result<()> {
    let Command::SaveObject(crate::commands::SaveObjectOp {
        id,
        kind,
        parent,
        data,
        reason,
        ..
    }) = command
    else {
        bail!("save_object required")
    };
    let parent = &parent
        .as_deref()
        .map(|key| resolve_address(w, key).to_owned());
    if let Some(cmd) = typed_command(w, kind, id, parent.as_deref(), data, reason)? {
        candidate.document_overrides.remove(key);
        commands::apply_inner(candidate, actor, &cmd, position, at)?;
        if let Command::Revise(crate::commands::ReviseOp { req, .. }) = &cmd
            && commands::rule_applies(candidate, commands::KEEP_ASSESSMENTS, position)
        {
            keep_assessments(candidate, req);
        }
        // A criterion save changes only that criterion: its siblings keep their revisions.
        let siblings = schema(w).types[kind].role != "ac"
            || !commands::rule_applies(candidate, commands::SINGLE_CRITERION_SAVE, position);
        // The specification revision names the criterion save that made it.
        let spec_reason = (schema(w).types[kind].role == "ac"
            && commands::rule_applies(candidate, commands::CRITERION_SAVE_REASON, position))
        .then(|| format!("Criterion {key} changed: {reason}"));
        sync_with(
            candidate,
            actor,
            &cmd,
            position,
            at,
            siblings,
            spec_reason.as_deref(),
        );
    } else {
        candidate.document_overrides.insert(key.to_owned());
        let owner = string(data, "owner")?;
        commands::human(w, &owner)?;
        if let Some(o) = old {
            ensure!(
                o.current().data["owner"] == owner
                    || o.current().data["owner"] == actor
                    || actor == w.admin,
                "only current owner can transfer responsibility"
            );
        }
        if ["project", "intake", "point"].contains(&kind.as_str()) && !w.schema_migrated {
            ensure!(
                old.is_some(),
                "create source data with save_object kind=note"
            );
            ensure!(
                actor == w.admin
                    || old
                        .as_ref()
                        .is_some_and(|o| o.current().data["owner"] == actor),
                "source owner required"
            );
            match kind.as_str() {
                "project" => {
                    ensure!(
                        string(data, "title")? == *id,
                        "project title is its stable identifier"
                    );
                    candidate
                        .projects
                        .insert(id.clone(), string(data, "repository")?);
                }
                "intake" => {
                    let i = candidate.intake.get_mut(id).context("unknown intake")?;
                    ensure!(
                        data["author"] == i.author && data["title"] == *id,
                        "source author and id cannot change"
                    );
                    i.said = string(data, "body")?;
                    i.owner = Some(owner.clone());
                }
                "point" => {
                    let (source, point) = id.split_once('/').context("invalid source point")?;
                    let p = candidate
                        .intake
                        .get_mut(source)
                        .and_then(|i| i.points.get_mut(point))
                        .context("unknown source point")?;
                    ensure!(
                        old.as_ref().unwrap().current().data["owner"] == owner,
                        "point inherits its source owner"
                    );
                    for field in ["kind", "answers", "outcome", "reason", "requirement"] {
                        ensure!(
                            data[field] == old.as_ref().unwrap().current().data[field],
                            "use source process commands for {field}"
                        );
                    }
                    p.said = string(data, "title")?;
                }
                _ => unreachable!(),
            }
            sync(candidate, actor, command, position, at);
        }
    }
    Ok(())
}

/// Reset the criteria below each object whose revision at `position` changed its title,
/// text, links or parent. Task and inbox revisions reset nothing. A blocking dependency
/// only orders work, so a change of those links alone resets nothing.
pub fn reset_changed(w: &mut Workspace, position: u64, at: &str) -> Result<()> {
    let types = schema(w).types;
    let migrated = w.test_rules_migrated;
    let content = |links: &[ObjectLink]| -> Vec<ObjectLink> {
        links
            .iter()
            .filter(|l| !(l.blocking && l.relation == "depends_on"))
            .cloned()
            .collect()
    };
    let changed: Vec<String> = w
        .objects
        .iter()
        .filter(|(_, o)| {
            o.current().position == position
                && types.get(&o.kind).is_some_and(|t| t.role != "task")
                && !input_type(w, &o.kind)
        })
        .filter(|(_, o)| {
            let current = o.current();
            o.revisions
                .iter()
                .rev()
                .find(|r| r.position < position)
                .is_some_and(|before| {
                    before.parent != current.parent
                        || content(&before.links) != content(&current.links)
                        || TEXT_FIELDS
                            .iter()
                            .any(|field| before.data.get(field) != current.data.get(field))
                        // A new test list resets the criterion. The migration moves a list
                        // into the document and changes no status.
                        || (migrated
                            && before.data.get("tests") != current.data.get("tests")
                            && current.reason != crate::test_rules::REASON)
                })
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in changed {
        reset_criteria(w, &key, at)?;
    }
    Ok(())
}

/// Reset each present criterion at and below `root` after a change recorded at `at`.
/// Reports received at or before `at` stop counting for it, and its readiness
/// assessment is cleared. A later boundary replaces an earlier one.
pub fn reset_criteria(w: &mut Workspace, root: &str, at: &str) -> Result<()> {
    let root = resolve_address(w, root).to_owned();
    let types = schema(w).types;
    let criteria: Vec<String> = w
        .objects
        .iter()
        .filter(|(key, o)| {
            o.current().present
                && types.get(&o.kind).is_some_and(|t| t.role == "ac")
                && within(w, key, &root)
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in criteria {
        if let Some((req, r, ac)) = criterion_context(w, &key) {
            let (req, number, id) = (req.to_owned(), r.number, ac.id.clone());
            let r = commands::revision(w, &req, number, true)?;
            let ac = r
                .acs
                .iter_mut()
                .find(|a| a.id == id)
                .context("unknown AC")?;
            if ac.assessment.take().is_some() {
                r.acceptance = None;
            }
        }
        // An earlier or equal time keeps the stored boundary.
        let later = w.criterion_boundaries.get(&key).is_some_and(|old| {
            use crate::ac_tests::Boundary;
            matches!(
                (Boundary::read(Some(old)), Boundary::read(Some(at))),
                (Boundary::After(old), Boundary::After(new)) if old >= new
            )
        });
        if !later {
            w.criterion_boundaries.insert(key, at.into());
        }
    }
    Ok(())
}

/// A Task pin stays current across process events, which keep its authored origin revision.
pub fn pin_current(w: &Workspace, target: &str, pin: Option<u32>) -> bool {
    let key = resolve_address(w, target);
    let (Some(pin), Some(o)) = (pin, w.objects.get(key)) else {
        return false;
    };
    // The last revisions that change only the test list keep a pin current.
    let (contract, current) = (contract_revision(o), o.current().revision);
    o.current().present
        && w.task_process
            .get(key)
            .map_or(contract <= pin && pin <= current, |t| {
                t.retired.is_none()
                    && t.origin_revision >= 1
                    && t.origin_revision <= pin
                    && pin <= current
            })
}
