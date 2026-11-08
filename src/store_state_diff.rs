//! The current rows that a write changes. A write changes few of the many objects and
//! requirements, so it serializes only those. The rows are the same as a comparison of the
//! `entries` of the whole workspace before and after the write.
use super::*;
use crate::model::Requirement;
use crate::objects::Object;

/// The rows that changed, and the keys of the rows that the write removed.
pub(crate) type Diff = (Entries, Vec<(String, String)>);

/// A copy of `w` without its objects and requirements, the two large sections of the rows.
/// The copy keeps every other field, also a field that a later change adds.
fn light(w: &Workspace) -> Workspace {
    let mut light = w.clone();
    light.objects.clear();
    light.requirements.clear();
    light
}

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

/// The row of an object or a requirement: its JSON with only the last revision.
fn row<T: serde::Serialize>(value: &T) -> Result<Value> {
    crate::probe::count(&crate::probe::ROWS);
    let mut value = serde_json::to_value(value)?;
    trim(&mut value)?;
    Ok(value)
}

/// Add the changed rows of one large section to `diff`. `same` skips the equal values without
/// a serialization. Two unequal values with the same row give no changed row.
fn section<T: serde::Serialize>(
    diff: &mut Diff,
    name: &str,
    new: &BTreeMap<String, T>,
    old: &BTreeMap<String, T>,
    same: impl Fn(&T, &T) -> bool,
) -> Result<()> {
    for (key, value) in new {
        let before = old.get(key);
        if before.is_some_and(|b| same(value, b)) {
            continue;
        }
        let value = row(value)?;
        if before.map(row).transpose()?.as_ref() != Some(&value) {
            diff.0.insert((name.into(), key.clone()), value);
        }
    }
    let removed = old.keys().filter(|key| !new.contains_key(*key));
    diff.1
        .extend(removed.map(|key| (name.to_owned(), key.clone())));
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
    section(
        &mut diff,
        "objects",
        &state.objects,
        &old.objects,
        same_object,
    )?;
    let (new, old) = (&state.requirements, &old.requirements);
    section(&mut diff, "requirements", new, old, same_requirement)?;
    Ok(diff)
}
