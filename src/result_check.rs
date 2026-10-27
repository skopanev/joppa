//! The check of one test result against the test rules of the schema.
use crate::result_shape::{
    FIELD_TYPES, MAX_FIELDS, MAX_TEXT, MAX_VALUES, RESERVED, ResultField, ResultShape,
    field_name_ok, text_ok,
};
use anyhow::{Result, bail, ensure};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// One result after the check against the block.
#[derive(Debug, PartialEq)]
pub struct Checked {
    /// The stored outcome: pass, fail, skipped or todo.
    pub status: &'static str,
    /// The status name that the record gave.
    pub status_name: Option<String>,
    /// The fields of the result, when it has some.
    pub fields: Option<Value>,
}

/// Check one result against the block and give the stored values. A text value longer
/// than its max_length keeps its first max_length characters.
pub fn check(
    shape: &ResultShape,
    status: &str,
    mut extra: Map<String, Value>,
    refs: &std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
) -> Result<Checked> {
    let Some(outcome) = shape.statuses.get(status) else {
        let list: Vec<&str> = shape.statuses.keys().map(String::as_str).collect();
        bail!("status must be one of: {}", list.join(", "));
    };
    for key in extra.keys() {
        ensure!(
            shape.fields.iter().any(|f| &f.name == key),
            "invalid record_results input: unknown result field `{key}`; the tree schema test_results lists the fields"
        );
    }
    for field in &shape.fields {
        let name = &field.name;
        let Some(value) = extra.get_mut(name) else {
            ensure!(!field.required, "result field {name} is required");
            continue;
        };
        let fits = match field.kind.as_str() {
            "number" => value.is_number(),
            "enum" => value
                .as_str()
                .is_some_and(|v| field.values.iter().any(|x| x == v)),
            // A ref field names a present object of its target type by id or address.
            "ref" => match value.as_str().map(|v| v.rsplit(':').next().unwrap_or(v)) {
                Some(id) if refs.get(name).is_some_and(|ids| ids.contains(id)) => {
                    *value = Value::String(id.to_owned());
                    true
                }
                _ => false,
            },
            _ => match value.as_str() {
                Some(text) if !text.is_empty() => {
                    let limit = field.max_length.unwrap_or(usize::MAX);
                    if text.chars().count() > limit {
                        *value = Value::String(text.chars().take(limit).collect());
                    }
                    true
                }
                _ => false,
            },
        };
        ensure!(
            fits,
            "result field {name} needs {}",
            match field.kind.as_str() {
                "number" => "a number".to_owned(),
                "enum" => format!("one of: {}", field.values.join(", ")),
                "ref" => format!(
                    "an object of the type {}",
                    field.target.as_deref().unwrap_or_default()
                ),
                _ => "a text that is not empty".to_owned(),
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
        status_name: Some(status.to_owned()),
        fields: (!extra.is_empty()).then_some(Value::Object(extra)),
    })
}

/// Refuse a bad field of the block.
pub(crate) fn validate_fields(fields: &[ResultField]) -> Result<()> {
    ensure!(
        fields.len() <= MAX_FIELDS,
        "test_results.fields has at most {MAX_FIELDS} fields"
    );
    let mut seen = BTreeSet::new();
    for field in fields {
        let name = field.name.as_str();
        ensure!(
            field_name_ok(name),
            "a result field name uses a-z, 0-9 and _, starts with a letter and has at most 40 characters"
        );
        ensure!(
            !RESERVED.contains(&name),
            "result field {name} is part of the record; use another name"
        );
        ensure!(seen.insert(name), "result field {name} appears twice");
        ensure!(
            field.title.as_deref().is_none_or(text_ok),
            "result field {name} needs a title of 1 to 1024 characters"
        );
        ensure!(
            FIELD_TYPES.contains(&field.kind.as_str()),
            "result field {name} has the type text, number, enum or ref"
        );
        ensure!(
            (field.kind == "ref") == field.target.as_deref().is_some_and(|t| !t.is_empty()),
            "ref field {name} needs a target type, and only a ref field has one"
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
            (field.kind == "text")
                == field
                    .max_length
                    .is_some_and(|m| (1..=MAX_TEXT).contains(&m)),
            "text field {name} needs max_length from 1 to {MAX_TEXT}, and only a text field has it"
        );
        ensure!(
            !field.board || field.kind == "enum" || field.kind == "ref",
            "board field {name} must be an enum or ref field"
        );
        ensure!(
            field.board || field.applies.is_none(),
            "only a board field has applies: {name}"
        );
    }
    Ok(())
}
