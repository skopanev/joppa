//! The data field types of a tree type: `fields` in set_tree_schema, such as
//! {"env": {"type": "enum", "values": ["dev", "prod"]}}. A save refuses a value of another
//! type, and set_tree_schema refuses a rule that a present object breaks. A field that the
//! rules do not name takes any value.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The types of a data field.
pub const TYPES: [&str; 5] = ["text", "number", "boolean", "enum", "list"];

/// The type of one data field: `enum` needs its values, and `list` may limit its elements.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FieldType {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
}

/// Refuse a rule with an unknown type, an enum without values, or values on another type.
pub fn validate_rule(field: &str, rule: &FieldType) -> Result<()> {
    let name = field.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && field.len() <= 32
        && field
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !["title", "body"].contains(&field);
    ensure!(
        name,
        "a typed field is a lowercase name of up to 32 characters, not title or body: {field}"
    );
    ensure!(
        TYPES.contains(&rule.kind.as_str()),
        "{field}: a field type is text, number, boolean, enum or list"
    );
    match rule.kind.as_str() {
        "enum" => ensure!(
            !rule.values.is_empty(),
            "{field}: an enum field needs values"
        ),
        "list" => {}
        _ => ensure!(
            rule.values.is_empty(),
            "{field}: only an enum or a list field has values"
        ),
    }
    Ok(())
}

/// The refusal of a value of `field`, or None when the rule accepts it. A null has no value.
pub fn refusal(field: &str, rule: &FieldType, value: &Value) -> Option<String> {
    let one_of = || format!("{field} is one of {}", rule.values.join(", "));
    let allowed = |text: &str| rule.values.is_empty() || rule.values.iter().any(|v| v == text);
    match (rule.kind.as_str(), value) {
        (_, Value::Null) => None,
        ("text", Value::String(_)) | ("number", Value::Number(_)) | ("boolean", Value::Bool(_)) => {
            None
        }
        ("enum", Value::String(text)) if allowed(text) => None,
        ("enum", _) => Some(one_of()),
        ("list", Value::Array(items)) if items.iter().all(|i| i.as_str().is_some_and(allowed)) => {
            None
        }
        ("list", _) if !rule.values.is_empty() => {
            Some(format!("{field} is a list of {}", rule.values.join(", ")))
        }
        ("list", _) => Some(format!("{field} is a list of texts")),
        ("boolean", _) => Some(format!("{field} is true or false")),
        ("number", _) => Some(format!("{field} is a number")),
        _ => Some(format!("{field} is a text")),
    }
}

/// The values of each typed enum field of the schema, the values of all types together, as
/// the board filter drop-downs offer them.
pub fn enum_values(w: &crate::model::Workspace) -> Vec<crate::filter_dimensions::Dimension> {
    let mut found: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for t in crate::objects::schema(w).types.values() {
        for (field, rule) in t.fields.iter().filter(|(_, r)| r.kind == "enum") {
            found
                .entry(field.clone())
                .or_default()
                .extend(rule.values.clone());
        }
    }
    found
        .into_iter()
        .map(|(field, mut values)| {
            values.sort();
            values.dedup();
            crate::filter_dimensions::Dimension { field, values }
        })
        .collect()
}
