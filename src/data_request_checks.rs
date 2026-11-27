//! Part of the request of `POST /api/data`: the checks of the type names, the filter values
//! and the numbers.
use super::bad;
use crate::objects::TreeType;
use anyhow::Result;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The field types of each declared field, from every type of the schema.
pub(super) fn declared(types: &BTreeMap<String, TreeType>) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for t in types.values() {
        for (field, rule) in &t.fields {
            out.entry(field.clone())
                .or_default()
                .insert(rule.kind.clone());
        }
    }
    out
}

/// A type name or a non-empty list of them. Each name must be a type of the schema.
pub(super) fn kinds(kind: &Value, types: &BTreeMap<String, TreeType>) -> Result<BTreeSet<String>> {
    let names: Vec<&Value> = match kind {
        Value::Array(items) if !items.is_empty() => items.iter().collect(),
        Value::String(_) => vec![kind],
        _ => {
            return Err(bad(
                "kind must be a type name or a non-empty list of type names",
            ));
        }
    };
    let mut out = BTreeSet::new();
    for name in names {
        let name = name
            .as_str()
            .ok_or_else(|| bad("kind must hold type names"))?;
        if !types.contains_key(name) {
            return Err(bad(format!("kind {name} is not a type of the schema")));
        }
        out.insert(name.to_owned());
    }
    Ok(out)
}

/// The wanted values of one filter field: one value or a non-empty list. Each value must
/// have the JSON type of one field type of `rules`.
pub(super) fn values(field: &str, wanted: Value, rules: &BTreeSet<String>) -> Result<Vec<Value>> {
    let items = match wanted {
        Value::Array(items) if items.is_empty() => {
            return Err(bad(format!("filter {field} must not be an empty list")));
        }
        Value::Array(items) => items,
        value => vec![value],
    };
    let fits = |rule: &String| {
        items.iter().all(|v| match rule.as_str() {
            "number" => v.is_number(),
            "boolean" => v.is_boolean(),
            _ => v.is_string(),
        })
    };
    if !rules.iter().any(fits) {
        let types = rules.iter().cloned().collect::<Vec<_>>().join(" or ");
        return Err(bad(format!(
            "filter {field} needs a {types} value or a list of them"
        )));
    }
    Ok(items)
}

/// `value` when it is 1 to `max`, else a refusal that names `name`.
pub(super) fn within(value: i64, max: i64, name: &str) -> Result<usize> {
    match (1..=max).contains(&value) {
        true => Ok(value as usize),
        false => Err(bad(format!("{name} must be 1 to {max}"))),
    }
}
