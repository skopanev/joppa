//! The current rows that a write changes. A write changes few of the many objects and
//! requirements, so it serializes only those. The rows are the same as a comparison of the
//! `entries` of the whole workspace before and after the write.
use super::light::light;
use super::*;
use crate::model::Requirement;
use crate::objects::Object;
use std::sync::Arc;

/// The rows that changed, and the keys of the rows that the write removed.
pub(crate) type Diff = (Entries, Vec<(String, String)>);

/// The meta row of `state`.
pub(crate) fn metadata(state: &Workspace) -> Result<String> {
    Ok(entries(&light(state))?
        .remove(&("meta".into(), String::new()))
        .unwrap()
        .to_string())
}

/// The two objects give the same row: the same fields and the same last revision.
fn same_object(new: &Object, old: &Object) -> bool {
    let Object {
        kind,
        id,
        revisions,
        parent_override,
    } = new;
    *kind == old.kind
        && *id == old.id
        && *parent_override == old.parent_override
        && revisions.last() == old.revisions.last()
}

/// The two requirements give the same row: the same fields and the same last revision.
fn same_requirement(new: &Requirement, old: &Requirement) -> bool {
    let Requirement {
        capability,
        revisions,
        comments,
    } = new;
    *capability == old.capability
        && *comments == old.comments
        && revisions.last() == old.revisions.last()
}

/// The row of a section that keeps the whole value.
fn plain<T: serde::Serialize>(value: &T) -> Result<Value> {
    Ok(serde_json::to_value(value)?)
}

/// The row of an object or a requirement: its JSON with only the last revision.
fn row<T: serde::Serialize>(value: &T) -> Result<Value> {
    crate::probe::count(&crate::probe::ROWS);
    let mut value = serde_json::to_value(value)?;
    trim(&mut value)?;
    Ok(value)
}

/// Add the changed rows of one large section to `diff`. `same` skips the equal values without
/// a serialization. Two unequal values with the same row give no changed row.
fn section<T>(
    diff: &mut Diff,
    name: &str,
    (new, old): (&BTreeMap<String, T>, &BTreeMap<String, T>),
    same: impl Fn(&T, &T) -> bool,
    row: impl Fn(&T) -> Result<Value>,
) -> Result<()> {
    for pair in crate::map_pairs::pairs(old, new) {
        let (key, before) = (pair.key, pair.old);
        let Some(value) = pair.new else {
            diff.1.push((name.to_owned(), key.to_owned()));
            continue;
        };
        if before.is_some_and(|b| same(value, b)) {
            continue;
        }
        let value = row(value)?;
        if before.map(&row).transpose()?.as_ref() != Some(&value) {
            diff.0.insert((name.into(), key.to_owned()), value);
        }
    }
    Ok(())
}

/// The rows of `state` that differ from the rows of `previous`, and the rows of `previous`
/// that `state` does not have. Without `previous`, every row of `state` changed.
pub(crate) fn changed_entries(state: &Workspace, previous: Option<&Workspace>) -> Result<Diff> {
    let current = entries(&light(state))?;
    let before = previous.map(|p| entries(&light(p))).transpose()?;
    let before = before.unwrap_or_default();
    let removed = before.keys().filter(|key| !current.contains_key(*key));
    let removed = removed.cloned().collect();
    let changed = current
        .into_iter()
        .filter(|(key, value)| before.get(key) != Some(value))
        .collect();
    let mut diff = (changed, removed);
    let empty = Workspace::default();
    let old = previous.unwrap_or(&empty);
    let objects = (&state.objects, &old.objects);
    let same = |new: &_, old: &_| std::sync::Arc::ptr_eq(new, old) || same_object(new, old);
    section(&mut diff, "objects", objects, same, row)?;
    let requirements = (&state.requirements, &old.requirements);
    section(
        &mut diff,
        "requirements",
        requirements,
        |new, old| Arc::ptr_eq(new, old) || same_requirement(new, old),
        row,
    )?;
    // These rows keep the whole value, and equal values need no serialization.
    let notes = (&state.notes, &old.notes);
    let same = |new: &Arc<_>, old: &Arc<_>| Arc::ptr_eq(new, old) || new == old;
    section(&mut diff, "notes", notes, same, plain)?;
    // A shared map that the write did not change is the same allocation.
    if !Arc::ptr_eq(&state.criterion_boundaries, &old.criterion_boundaries) {
        let boundaries = (&*state.criterion_boundaries, &*old.criterion_boundaries);
        section(
            &mut diff,
            "criterion_boundaries",
            boundaries,
            PartialEq::eq,
            plain,
        )?;
    }
    if !Arc::ptr_eq(&state.former_test_titles, &old.former_test_titles) {
        let titles = (&*state.former_test_titles, &*old.former_test_titles);
        section(
            &mut diff,
            "former_test_titles",
            titles,
            PartialEq::eq,
            plain,
        )?;
    }
    // Each address of a set is one row with the value true (store_state_current.rs).
    for (name, new, old) in [
        (
            "document_overrides",
            &state.document_overrides,
            &old.document_overrides,
        ),
        (
            "archived_objects",
            &state.archived_objects,
            &old.archived_objects,
        ),
    ] {
        if Arc::ptr_eq(new, old) {
            continue;
        }
        for key in new.difference(old) {
            diff.0.insert((name.into(), key.clone()), Value::Bool(true));
        }
        diff.1.extend(
            old.difference(new)
                .map(|key| (name.to_owned(), key.clone())),
        );
    }
    Ok(diff)
}
