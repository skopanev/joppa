//! The filter dimensions of a workspace, such as env or asset. A dimension is a filter field
//! of the tree schema (`filters`) that a test result field of type enum or ref also names.
//! - Its values are the enum values, or the addresses of the present objects of the ref
//!   target type.
//! - A node applies to the values of its own `data.<field>` (a text or a list of texts), else
//!   to the values of its nearest ancestor that has the field, else to every value.
//! - A result without the field counts for every value of the dimension.
//! - A combination names one value of one or more dimensions. The status projection stores the
//!   tone of each node for each combination, at most MAX_COMBINATIONS of them.
use crate::model::Workspace;
use crate::objects::{resolve_address, schema};
use serde_json::Value;
use std::collections::BTreeMap;

/// The largest number of combinations that the status projection stores.
pub const MAX_COMBINATIONS: usize = 64;

/// One dimension: the field name and its values in text order.
#[derive(Clone, Debug, PartialEq)]
pub struct Dimension {
    pub field: String,
    pub values: Vec<String>,
}

/// A combination: one value for each named dimension.
pub type Combination = BTreeMap<String, String>;

/// The dimensions of the workspace, in the order of `filters`.
pub fn dimensions(w: &Workspace) -> Vec<Dimension> {
    let filters = schema(w).filters.unwrap_or_default();
    let Ok(rules) = crate::test_rules::rules(w) else {
        return vec![];
    };
    filters
        .iter()
        .filter_map(|field| {
            let result = rules.fields.iter().find(|f| &f.name == field)?;
            let mut values: Vec<String> = match result.kind.as_str() {
                "enum" => result.values.clone(),
                "ref" => {
                    let target = result.target.as_deref()?;
                    let present = w.objects.iter().filter(|(_, o)| o.current().present);
                    present
                        .filter(|(_, o)| o.kind == target)
                        .map(|(address, _)| address.clone())
                        .collect()
                }
                _ => return None,
            };
            values.sort();
            values.dedup();
            Some(Dimension {
                field: field.clone(),
                values,
            })
        })
        .collect()
}

/// The combinations of the dimensions: each one names one value of one dimension or more.
/// Combinations of fewer dimensions come first, and at most MAX_COMBINATIONS stay.
pub fn combinations(dimensions: &[Dimension]) -> Vec<Combination> {
    let mut all: Vec<Combination> = vec![Combination::new()];
    for dimension in dimensions {
        let mut next = vec![];
        for combination in &all {
            next.push(combination.clone());
            for value in &dimension.values {
                let mut more = combination.clone();
                more.insert(dimension.field.clone(), value.clone());
                next.push(more);
            }
        }
        all = next;
    }
    all.retain(|c| !c.is_empty());
    all.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    all.truncate(MAX_COMBINATIONS);
    all
}

/// The stored key of a combination: its JSON text, such as {"env":"dev"}.
pub fn key(combination: &Combination) -> String {
    serde_json::to_string(combination).unwrap_or_default()
}

/// The values that each node applies to, for each dimension that the node or an ancestor
/// names. A node without an entry applies to every value.
pub fn effective(
    w: &Workspace,
    dimensions: &[Dimension],
) -> BTreeMap<String, BTreeMap<String, Vec<String>>> {
    let mut out = BTreeMap::new();
    for (address, _) in w.objects.iter().filter(|(_, o)| o.current().present) {
        let mut found = BTreeMap::new();
        for dimension in dimensions {
            if let Some(values) = applied(w, address, &dimension.field) {
                found.insert(dimension.field.clone(), values);
            }
        }
        if !found.is_empty() {
            out.insert(address.clone(), found);
        }
    }
    out
}

/// The values of `field` of the node or of its nearest ancestor with the field.
fn applied(w: &Workspace, address: &str, field: &str) -> Option<Vec<String>> {
    let mut current = Some(address.to_owned());
    let mut seen = std::collections::BTreeSet::new();
    while let Some(key) = current {
        let key = resolve_address(w, &key).to_owned();
        if !seen.insert(key.clone()) {
            return None;
        }
        let revision = w.objects.get(&key)?.current();
        match &revision.data[field] {
            Value::String(value) => return Some(vec![value.clone()]),
            Value::Array(items) => {
                return Some(
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect(),
                );
            }
            _ => current = revision.parent.clone(),
        }
    }
    None
}

/// True when a node with these effective values applies to the combination.
pub fn applies(values: Option<&BTreeMap<String, Vec<String>>>, combination: &Combination) -> bool {
    combination.iter().all(|(field, value)| {
        values
            .and_then(|v| v.get(field))
            .is_none_or(|values| values.contains(value))
    })
}

/// True when a result counts for the combination: each dimension field of the result is
/// absent or names the value.
pub fn counts(fields: Option<&Value>, combination: &Combination) -> bool {
    combination.iter().all(|(field, value)| {
        match fields.and_then(|f| f.get(field)).filter(|v| !v.is_null()) {
            None => true,
            Some(Value::String(found)) => found == value,
            Some(_) => false,
        }
    })
}

/// The combination of a data filter: the values of its dimension fields, and the rest of the
/// filter. A list value names its one element.
pub fn split(data: &Value, fields: &[String]) -> (Combination, Value) {
    let mut rest = data.clone();
    let mut combination = Combination::new();
    for field in fields {
        let value = match rest.get(field) {
            Some(Value::String(v)) => Some(v.clone()),
            Some(Value::Array(items)) if items.len() == 1 => items[0].as_str().map(String::from),
            _ => None,
        };
        if let Some(value) = value {
            combination.insert(field.clone(), value);
            rest.as_object_mut().map(|m| m.remove(field));
        }
    }
    (combination, rest)
}

/// The board filter values with the values of each dimension: a dimension field offers each of
/// its values, also a value that no node names.
pub fn with_values(mut filters: Value, dimensions: &[Dimension]) -> Value {
    for entry in filters.as_array_mut().into_iter().flatten() {
        let field = entry["field"].as_str().unwrap_or_default();
        if let Some(dimension) = dimensions.iter().find(|d| d.field == field) {
            let values: Vec<Value> = dimension
                .values
                .iter()
                .map(|v| serde_json::json!({"value":v,"list":false}))
                .collect();
            entry["values"] = Value::Array(values);
        }
    }
    filters
}

/// The dimensions as the board read gives them: `[{field, values}]`.
pub fn listed(dimensions: &[Dimension]) -> Value {
    let list: Vec<Value> = dimensions
        .iter()
        .map(|d| serde_json::json!({"field":d.field,"values":d.values}))
        .collect();
    Value::Array(list)
}
