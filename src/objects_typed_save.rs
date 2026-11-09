//! Part of `objects`: `typed_save` and the items after it, moved from objects.rs unchanged.
use super::*;

/// Apply the typed command of a saved document, or keep the document as an override.
#[allow(clippy::too_many_arguments)]
pub(crate) fn typed_save(
    w: &mut Workspace,
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
    let criterion = schema(w).types[kind].role == "ac";
    if let Some(cmd) = typed_command(w, kind, id, parent.as_deref(), data, reason)? {
        // A criterion that moves to another requirement leaves the old one first.
        let out = criterion_move_out(w, kind, id, &cmd, reason)?;
        w.document_overrides.remove(key);
        if let Some(out) = out {
            commands::apply_inner(w, actor, &out, position, at)?;
        }
        commands::apply_inner(w, actor, &cmd, position, at)?;
        // A criterion save changes only that criterion: its siblings keep their revisions.
        let siblings =
            !criterion || !commands::rule_applies(w, commands::SINGLE_CRITERION_SAVE, position);
        // The specification revision names the criterion save that made it.
        let spec_reason = (criterion
            && commands::rule_applies(w, commands::CRITERION_SAVE_REASON, position))
        .then(|| format!("Criterion {key} changed: {reason}"));
        sync_with(
            w,
            actor,
            &cmd,
            position,
            at,
            siblings,
            spec_reason.as_deref(),
        );
    } else {
        w.document_overrides.insert(key.to_owned());
        let owner = string(data, "owner")?;
        commands::human(w, &owner)?;
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
                    w.projects.insert(id.clone(), string(data, "repository")?);
                }
                "intake" => {
                    let i = w.intake.get_mut(id).context("unknown intake")?;
                    ensure!(
                        data["author"] == i.author && data["title"] == *id,
                        "source author and id cannot change"
                    );
                    i.said = string(data, "body")?;
                    i.owner = Some(owner.clone());
                }
                "point" => {
                    let (source, point) = id.split_once('/').context("invalid source point")?;
                    let p = w
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
            sync(w, actor, command, position, at);
        }
    }
    Ok(())
}

/// Reset the criteria below each object whose revision at `position` changed it. From the epoch
/// of any_change_reset, the object that `command` saves or links resets on any new parent,
/// edge or data value. Every other revision, such as a revision that a save gives to another
/// object, resets only on a new title, text, related edge or parent, and a Task or an inbox
/// revision resets nothing. From the epoch of relations_reset, only the depends_on and related
/// edges count and a flow never resets; before it, a revision compares its older links.
pub fn reset_changed(w: &mut Workspace, command: &Command, position: u64, at: &str) -> Result<()> {
    let types = schema(w).types;
    let migrated = w.test_rules_migrated;
    // The replay of reset_changed runs after the replaying flag is cleared: read the epoch.
    let edited = match command {
        Command::SaveObject(crate::commands::SaveObjectOp { kind, id, .. }) => {
            Some(resolve_address(w, &address(kind, id)).to_owned())
        }
        Command::SetRemoteLinks { target, .. } => Some(resolve_address(w, target).to_owned()),
        _ => None,
    }
    .filter(|_| commands::epoch_reached(w, commands::ANY_CHANGE_RESET, position));
    let any = |key: &str| edited.as_deref() == Some(key);
    // From the epoch of rename_keeps_results, a new title of a test node changes no test.
    let renames = commands::epoch_reached(w, commands::RENAME_KEEPS_RESULTS, position);
    if renames {
        crate::test_nodes::keep_former_titles(w, &types, position);
    }
    let quiet = |kind: &str| {
        renames
            && types
                .get(kind)
                .is_some_and(|t| t.role == crate::test_nodes::ROLE)
    };
    let relations = commands::epoch_reached(w, commands::RELATIONS_RESET, position);
    let content = |links: &[ObjectLink]| -> Vec<ObjectLink> {
        links
            .iter()
            .filter(|l| !(l.blocking && l.relation == "depends_on"))
            .cloned()
            .collect()
    };
    // Whether the edges changed, and whether the edges other than the dependencies changed.
    // A revision of an event with links, also one that the older release wrote after the
    // epoch, compares by the older rule.
    let edges = |before: &ObjectRevision, current: &ObjectRevision| -> (bool, bool) {
        if relations && current.older_links.0.is_none() {
            use crate::relations::{DEPENDS_ON, RELATED, edges_of};
            let same = |key| edges_of(&before.relations, key).eq(edges_of(&current.relations, key));
            let related = !same(RELATED);
            (related || !same(DEPENDS_ON), related)
        } else {
            let (old, new) = (before.link_form(), current.link_form());
            (old != new, content(&old) != content(&new))
        }
    };
    let changed: Vec<String> = w
        .objects
        .iter()
        .filter(|(key, o)| {
            o.current().position == position
                && (any(key)
                    || types.get(&o.kind).is_some_and(|t| t.role != "task")
                        && !input_type(w, &o.kind))
        })
        .filter(|(key, o)| {
            let current = o.current();
            let untitled = crate::test_nodes::untitled;
            o.revisions
                .iter()
                .rev()
                .find(|r| r.position < position)
                .is_some_and(|before| {
                    let quiet = quiet(&o.kind);
                    let (data, old) = (
                        untitled(&current.data, quiet),
                        untitled(&before.data, quiet),
                    );
                    let (links, related) = edges(before, current);
                    // From the epoch on, any new parent, edge or data value resets. A revision
                    // that changes none of them, such as a specification revision, does not.
                    (any(key) && (before.parent != current.parent || links || old != data))
                        || before.parent != current.parent
                        || related
                        || !crate::body_migration::same_text(w, &old, &data)
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

/// Reset each present criterion and test node at and below `root` after a change recorded at `at`.
/// Reports received at or before `at` stop counting for it. A later boundary replaces an
/// earlier one.
pub fn reset_criteria(w: &mut Workspace, root: &str, at: &str) -> Result<()> {
    let root = resolve_address(w, root).to_owned();
    let types = schema(w).types;
    let criteria: Vec<String> = w
        .objects
        .iter()
        .filter(|(key, o)| {
            o.current().present
                && types
                    .get(&o.kind)
                    .is_some_and(|t| t.role == "ac" || t.role == crate::test_nodes::ROLE)
                && within(w, key, &root)
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in criteria {
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
