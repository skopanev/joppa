//! The reset of the criteria of a source. A node of a type with `source: true` that gets a new
//! title, a new body or a new file resets each present criterion (a type with the role ac)
//! that names it in one of its reference fields, or that has a link to it or from it with any
//! relation. A detach and a change of another data field reset nothing. Every live write
//! applies the rule, and a replay from the epoch of source_change_reset on.
use crate::commands::Command;
use crate::model::Workspace;
use crate::objects::{ObjectRevision, TreeType, reset_criteria, resolve_address, schema};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

/// Reset the criteria of the source that `command` changed at `position`.
pub fn reset(w: &mut Workspace, command: &Command, position: u64, at: &str) -> Result<()> {
    let changed = match command {
        Command::Attach(op) => resolve_address(w, &op.target).to_owned(),
        Command::SaveObject(op) => {
            let key = resolve_address(w, &format!("{}:{}", op.kind, op.id)).to_owned();
            if !new_text(w, &key, position) {
                return Ok(());
            }
            key
        }
        _ => return Ok(()),
    };
    let types = schema(w).types;
    let Some(source) = w
        .objects
        .get(&changed)
        .filter(|o| types.get(&o.kind).is_some_and(|t| t.source))
    else {
        return Ok(());
    };
    let criterion = |key: &str| {
        w.objects.get(key).is_some_and(|o| {
            o.current().present && types.get(&o.kind).is_some_and(|t| t.role == "ac")
        })
    };
    // The criteria that the source links to, and the criteria that link to it or name it.
    let mut criteria: BTreeSet<String> = source
        .current()
        .links
        .iter()
        .map(|l| resolve_address(w, &l.target).to_owned())
        .filter(|key| criterion(key))
        .collect();
    for (key, o) in &w.objects {
        let current = o.current();
        if criterion(key)
            && (current
                .links
                .iter()
                .any(|l| resolve_address(w, &l.target) == changed)
                || names(w, &types, &o.kind, current, &changed))
        {
            criteria.insert(key.clone());
        }
    }
    for key in criteria {
        reset_criteria(w, &key, at)?;
    }
    Ok(())
}

/// The revision at `position` gives the node at `key` a new title or a new body.
fn new_text(w: &Workspace, key: &str, position: u64) -> bool {
    let Some(o) = w.objects.get(key) else {
        return false;
    };
    let current = o.current();
    let before = o.revisions.iter().rev().find(|r| r.position < position);
    current.position == position
        && before.is_some_and(|before| {
            ["title", "body"]
                .iter()
                .any(|field| before.data.get(field) != current.data.get(field))
        })
}

/// A reference field of the type of the revision names `target`. A value without a kind names
/// an object of the target type of its field.
fn names(
    w: &Workspace,
    types: &BTreeMap<String, TreeType>,
    kind: &str,
    revision: &ObjectRevision,
    target: &str,
) -> bool {
    let references = types.get(kind).map(|t| &t.references);
    references.into_iter().flatten().any(|(field, rule)| {
        let values = match &revision.data[field] {
            serde_json::Value::String(value) => vec![value.as_str()],
            serde_json::Value::Array(values) => values.iter().filter_map(|v| v.as_str()).collect(),
            _ => vec![],
        };
        values.into_iter().any(|value| {
            let address = if value.contains(':') {
                value.to_owned()
            } else {
                format!("{}:{value}", rule.target)
            };
            resolve_address(w, &address) == target
        })
    })
}
