//! One consumer per criterion. A criterion names the one consumer that it serves, so a
//! green criterion closes one consumer without doubt. A requirement and every level above
//! it name no consumer.
//!
//! The migrate_consumers event converts a workspace once. It gives the tree schema one
//! revision: each criterion type gets the required single reference `consumer`, and the
//! criterion and requirement types lose their references to the built-in Consumer type.
//! Each live criterion then gets one new revision with the first valid consumer of its
//! old list, else of its requirement. The text, the tests and the readiness of the
//! criterion do not change, so its status stays. A criterion without a valid consumer
//! keeps its document and needs a consumer at its next save.
use crate::model::Workspace;
use crate::objects::{ReferenceRule, TreeType, resolve_address, schema};
use anyhow::{Result, ensure};
use serde_json::Value;
use std::collections::BTreeMap;

/// The built-in Consumer type. A workspace without it keeps its tree.
pub const KIND: &str = "consumer";
/// The single reference of a criterion to its consumer.
pub const FIELD: &str = "consumer";
/// The reason of the migration schema revision. The built-in tree stays built-in after it.
pub const REASON: &str =
    "One consumer per criterion: the criterion names its consumer, a requirement names none";

fn rule() -> ReferenceRule {
    ReferenceRule {
        target: KIND.into(),
        title: "Used by".into(),
        required: true,
        multiple: false,
        verification_subject: false,
    }
}

/// Convert the types in place. Return, per kind, the old fields that named a consumer.
pub fn convert_types(types: &mut BTreeMap<String, TreeType>) -> BTreeMap<String, Vec<String>> {
    let mut old = BTreeMap::new();
    if !types.contains_key(KIND) {
        return old;
    }
    for (kind, t) in types.iter_mut() {
        if !matches!(t.role.as_str(), "ac" | "requirement") {
            continue;
        }
        let fields: Vec<String> = t
            .references
            .iter()
            .filter(|(field, r)| {
                r.target == KIND && !(t.role == "ac" && field.as_str() == FIELD && **r == rule())
            })
            .map(|(field, _)| field.clone())
            .collect();
        for field in &fields {
            t.references.remove(field);
        }
        if t.role == "ac" {
            t.references.insert(FIELD.into(), rule());
        }
        old.insert(kind.clone(), fields);
    }
    old
}

/// The consumer IDs that a document lists in these fields, in their order.
fn listed(data: &Value, fields: &[String]) -> Vec<String> {
    fields
        .iter()
        .flat_map(|field| match data.get(field) {
            Some(Value::String(id)) => vec![id.clone()],
            Some(Value::Array(ids)) => ids
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
            _ => vec![],
        })
        .collect()
}

/// Whether `id` names a live object of the Consumer type.
pub fn valid(w: &Workspace, id: &str) -> bool {
    let target = if id.contains(':') {
        id.to_owned()
    } else {
        format!("{KIND}:{id}")
    };
    let target = resolve_address(w, &target);
    w.objects
        .get(target)
        .is_some_and(|o| o.kind == KIND && o.current().present)
}

/// Apply migrate_consumers once. The workspace owner is the actor of the event.
pub fn migrate(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.consumers_migrated {
        return Ok(());
    }
    ensure!(actor == w.admin, "workspace owner required");
    w.consumers_migrated = true;
    let mut next = schema(w);
    let before = next.types.clone();
    let old = convert_types(&mut next.types);
    if next.types == before {
        return Ok(());
    }
    next.revision += 1;
    next.actor = actor.into();
    next.at = at.into();
    next.position = position;
    next.reason = REASON.into();
    let schema_revision = next.revision;
    let criteria: Vec<String> = next
        .types
        .iter()
        .filter(|(_, t)| t.role == "ac")
        .map(|(k, _)| k.clone())
        .collect();
    w.tree_schema.push(next);
    let keys: Vec<String> = w
        .objects
        .iter()
        .filter(|(_, o)| criteria.contains(&o.kind) && o.current().present)
        .map(|(key, _)| key.clone())
        .collect();
    for key in keys {
        let o = &w.objects[&key];
        let current = o.current();
        if current
            .data
            .get(FIELD)
            .and_then(Value::as_str)
            .is_some_and(|id| valid(w, id))
        {
            continue;
        }
        let fields = old.get(&o.kind).cloned().unwrap_or_default();
        let parent = current
            .parent
            .as_deref()
            .map(|p| resolve_address(w, p).to_owned());
        let from_parent = parent
            .as_ref()
            .and_then(|p| w.objects.get(p))
            .map(|p| {
                listed(
                    &p.current().data,
                    old.get(&p.kind).map(Vec::as_slice).unwrap_or_default(),
                )
            })
            .unwrap_or_default();
        let Some(consumer) = listed(&current.data, &fields)
            .into_iter()
            .chain(from_parent)
            .find(|id| valid(w, id))
        else {
            continue;
        };
        let mut revision = current.clone();
        if let Some(data) = revision.data.as_object_mut() {
            for field in &fields {
                data.remove(field);
            }
            data.insert(FIELD.into(), Value::String(consumer));
        }
        revision.revision += 1;
        revision.schema_revision = schema_revision;
        revision.position = position;
        revision.actor = actor.into();
        revision.reason = REASON.into();
        revision.at = at.into();
        w.objects
            .get_mut(&key)
            .expect("criterion")
            .revisions
            .push(revision);
    }
    Ok(())
}

/// A legacy revise names its consumers on the requirement. A criterion that the revise
/// writes without a consumer takes the first consumer of its requirement.
pub fn fill(t: &TreeType, data: &mut Value, requirement: Option<&Value>) {
    let single = t
        .references
        .get(FIELD)
        .is_some_and(|r| r.target == KIND && !r.multiple);
    if t.role != "ac" || !single || data.get(FIELD).is_some_and(|v| !v.is_null()) {
        return;
    }
    if let Some(first) = requirement
        .and_then(|r| r.get("consumers"))
        .and_then(Value::as_array)
        .and_then(|ids| ids.first())
        .and_then(Value::as_str)
    {
        data[FIELD] = Value::String(first.to_owned());
    }
}

/// A legacy revise gives each new criterion the first consumer of the revise. Refuse a
/// revise that adds a criterion without a consumer while no listed consumer is valid.
pub fn check_revise(w: &Workspace, consumers: &[String], acs: &[crate::model::Ac]) -> Result<()> {
    let types = schema(w).types;
    let needs = acs.iter().any(|ac| {
        let key = crate::objects::criterion_address(&ac.id);
        let kind = key.split_once(':').map_or("", |(kind, _)| kind);
        types
            .get(kind)
            .is_some_and(|t| t.references.contains_key(FIELD))
            && w.objects.get(resolve_address(w, &key)).is_none_or(|o| {
                !o.current().present
                    || o.current()
                        .data
                        .get(FIELD)
                        .and_then(Value::as_str)
                        .is_none_or(|id| !valid(w, id))
            })
    });
    ensure!(
        !needs || consumers.iter().any(|id| valid(w, id)),
        "reference consumer is required"
    );
    Ok(())
}
