//! The shape of a test result in the tree schema: the status names and the extra fields.
//!
//! A schema without `test_results` uses the built-in shape: the status is pass, fail, skip
//! or todo, and a result has an optional message and duration_ms. A saved shape can give
//! its own status names. Each name counts as one outcome: pass, fail, skip or todo. A saved
//! shape can also add fields of the type text, number or enum. The fields test, status,
//! message and duration_ms stay built-in, and commit stays on the record.
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The outcomes that a status name can count as.
pub const OUTCOMES: [&str; 4] = ["pass", "fail", "skip", "todo"];
/// Field names that the record or the server already uses.
const RESERVED: [&str; 7] = [
    "test",
    "status",
    "message",
    "duration_ms",
    "commit",
    "actor",
    "at",
];
const FIELD_TYPES: [&str; 3] = ["text", "number", "enum"];
const MAX_FIELDS: usize = 20;
const MAX_STATUSES: usize = 20;
const MAX_VALUES: usize = 50;
/// A text value or an enum value keeps at most this number of characters.
const TEXT_MAX: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultShape {
    /// Each accepted status name with the outcome that it counts as. Without it the
    /// names are pass, fail, skip and todo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statuses: Option<BTreeMap<String, String>>,
    /// The extra fields of a result, in the order that readers show them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<ResultField>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultField {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// One of text, number or enum.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    /// The permitted values of an enum field. Other types have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    /// The board filters by this field: each value gets its own status. Only a required
    /// enum field can set it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub board: bool,
}

/// One result after the check against the shape.
#[derive(Debug, PartialEq)]
pub struct Checked {
    /// The stored outcome: pass, fail, skipped or todo.
    pub status: &'static str,
    /// The status name of a shape with its own names.
    pub status_name: Option<String>,
    /// The extra fields, when the result has some.
    pub fields: Option<Value>,
}

fn name_ok(name: &str) -> bool {
    (1..=40).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn text_ok(value: &str) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= TEXT_MAX
        && !value.chars().any(char::is_control)
}

/// Refuse a shape with a bad status name, an unknown outcome or a bad field.
pub fn validate(shape: &ResultShape) -> Result<()> {
    if let Some(statuses) = &shape.statuses {
        ensure!(
            (1..=MAX_STATUSES).contains(&statuses.len()),
            "test_results.statuses has 1 to {MAX_STATUSES} names"
        );
        for (name, outcome) in statuses {
            ensure!(
                (1..=32).contains(&name.chars().count())
                    && name.chars().all(|c| !c.is_whitespace() && !c.is_control()),
                "a test status name has 1 to 32 characters without spaces"
            );
            ensure!(
                OUTCOMES.contains(&outcome.as_str()),
                "status {name} must count as pass, fail, skip or todo"
            );
        }
        for needed in ["pass", "fail"] {
            ensure!(
                statuses.values().any(|o| o == needed),
                "test_results.statuses needs a name that counts as {needed}"
            );
        }
    }
    ensure!(
        shape.fields.len() <= MAX_FIELDS,
        "test_results.fields has at most {MAX_FIELDS} fields"
    );
    let mut seen = BTreeSet::new();
    for field in &shape.fields {
        let name = field.name.as_str();
        ensure!(
            name_ok(name),
            "a result field name uses a-z, 0-9 and _, starts with a letter and has at most 40 characters"
        );
        ensure!(
            !RESERVED.contains(&name),
            "result field {name} is built-in; use another name"
        );
        ensure!(seen.insert(name), "result field {name} appears twice");
        ensure!(
            field.title.as_deref().is_none_or(text_ok),
            "result field {name} needs a title of 1 to {TEXT_MAX} characters"
        );
        ensure!(
            FIELD_TYPES.contains(&field.kind.as_str()),
            "result field {name} has the type text, number or enum"
        );
        if field.kind == "enum" {
            let mut values = BTreeSet::new();
            ensure!(
                (1..=MAX_VALUES).contains(&field.values.len())
                    && field
                        .values
                        .iter()
                        .all(|v| text_ok(v) && values.insert(v.as_str())),
                "enum field {name} needs 1 to {MAX_VALUES} different values"
            );
        } else {
            ensure!(
                field.values.is_empty(),
                "only an enum field has values: {name}"
            );
        }
        ensure!(
            !field.board || (field.kind == "enum" && field.required),
            "board field {name} must be a required enum field"
        );
    }
    Ok(())
}

/// The board fields of the shape, each with its values in the schema order.
pub fn board_fields(shape: Option<&ResultShape>) -> Vec<(String, Vec<String>)> {
    shape
        .map(|s| s.fields.as_slice())
        .unwrap_or_default()
        .iter()
        .filter(|f| f.board)
        .map(|f| (f.name.clone(), f.values.clone()))
        .collect()
}

/// The status names that apply, each with its outcome.
pub fn statuses(shape: Option<&ResultShape>) -> BTreeMap<String, String> {
    shape
        .and_then(|s| s.statuses.clone())
        .unwrap_or_else(|| OUTCOMES.map(|o| (o.to_owned(), o.to_owned())).into())
}

/// The shape that applies, for readers: the status names and the fields.
pub fn effective(shape: Option<&ResultShape>) -> Value {
    json!({
        "statuses": statuses(shape),
        "fields": shape.map(|s| s.fields.clone()).unwrap_or_default(),
    })
}

/// Check one result against the shape and give the stored values.
pub fn check(
    shape: Option<&ResultShape>,
    status: &str,
    extra: Map<String, Value>,
) -> Result<Checked> {
    let named = shape.is_some_and(|s| s.statuses.is_some());
    let names = statuses(shape);
    let Some(outcome) = names.get(status) else {
        if named {
            let list: Vec<&str> = names.keys().map(String::as_str).collect();
            bail!("status must be one of: {}", list.join(", "));
        }
        bail!("status must be pass, fail, skip or todo");
    };
    let declared = shape.map(|s| s.fields.as_slice()).unwrap_or_default();
    for key in extra.keys() {
        ensure!(
            declared.iter().any(|f| &f.name == key),
            "invalid record_results input: unknown result field `{key}`; the tree schema test_results lists the fields"
        );
    }
    for field in declared {
        let name = &field.name;
        let Some(value) = extra.get(name) else {
            ensure!(!field.required, "result field {name} is required");
            continue;
        };
        let fits = match field.kind.as_str() {
            "number" => value.is_number(),
            "enum" => value
                .as_str()
                .is_some_and(|v| field.values.iter().any(|x| x == v)),
            _ => value.as_str().is_some_and(text_ok),
        };
        ensure!(
            fits,
            "result field {name} needs {}",
            match field.kind.as_str() {
                "number" => "a number".to_owned(),
                "enum" => format!("one of: {}", field.values.join(", ")),
                _ => format!("a text of 1 to {TEXT_MAX} characters"),
            }
        );
    }
    Ok(Checked {
        status: match outcome.as_str() {
            "pass" => "pass",
            "fail" => "fail",
            "skip" => "skipped",
            _ => "todo",
        },
        status_name: named.then(|| status.to_owned()),
        fields: (!extra.is_empty()).then_some(Value::Object(extra)),
    })
}
