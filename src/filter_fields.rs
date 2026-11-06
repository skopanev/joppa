//! The filter fields of the tree schema: the `data` fields that the board offers as filter
//! drop-downs. `set_tree_schema` saves them in `filters`, and a command without `filters`
//! keeps the saved list. The board projection stores the present values of each field. A
//! value is a string field, or a string element of a list field such as `tags`. A choice
//! filters with the `data` node filter: `{"tags":["swap"]}` or `{"priority":"p0"}`.
use crate::model::Workspace;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// The largest number of filter fields, and of values that the board offers for one field.
pub const MAX_FIELDS: usize = 16;
pub const MAX_VALUES: usize = 200;

pub const REFUSAL: &str = "filters names 0 to 16 different data fields: a lowercase letter, then up to 31 lowercase letters, digits or _; not title or body";

/// Refuse a list that names too many fields, a field twice, a bad name, title or body.
pub fn validate(fields: &[String]) -> Result<()> {
    let mut seen = BTreeSet::new();
    ensure!(
        fields.len() <= MAX_FIELDS
            && fields
                .iter()
                .all(|f| name(f) && !["title", "body"].contains(&f.as_str()) && seen.insert(f)),
        REFUSAL
    );
    Ok(())
}

fn name(field: &str) -> bool {
    let mut chars = field.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && field.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The present values of each filter field: `[{field, values: [{value, list}]}]`. `list` is
/// true for an element of a list field. A boolean field gives false and true first, then the
/// texts in text order, at most MAX_VALUES in all.
pub fn values(state: &Workspace, fields: &[String]) -> Value {
    let present: Vec<&Value> = state
        .objects
        .values()
        .filter(|o| o.current().present)
        .map(|o| &o.current().data)
        .collect();
    let entries: Vec<Value> = fields
        .iter()
        .map(|field| {
            let (mut found, mut flags) = (BTreeSet::new(), BTreeSet::new());
            for data in &present {
                match &data[field] {
                    Value::Bool(flag) => {
                        flags.insert(*flag);
                    }
                    Value::String(text) => {
                        found.insert((text.clone(), false));
                    }
                    Value::Array(items) => {
                        for text in items.iter().filter_map(Value::as_str) {
                            found.insert((text.to_owned(), true));
                        }
                    }
                    _ => {}
                }
            }
            let flags = flags
                .into_iter()
                .map(|flag| json!({"value":flag,"list":false}));
            let texts = found
                .into_iter()
                .map(|(value, list)| json!({"value":value,"list":list}));
            let values: Vec<Value> = flags.chain(texts).take(MAX_VALUES).collect();
            json!({"field":field,"values":values})
        })
        .collect();
    json!(entries)
}

/// The present values of data.assignee, a member ID or an agent alias, in text order and at
/// most MAX_VALUES. The board assignee picker offers them next to the members.
pub fn assignees(state: &Workspace) -> Vec<String> {
    let found: BTreeSet<&str> = state
        .objects
        .values()
        .map(|o| o.current())
        .filter(|r| r.present)
        .filter_map(|r| r.data["assignee"].as_str().filter(|a| !a.is_empty()))
        .collect();
    found
        .into_iter()
        .take(MAX_VALUES)
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_list_takes_16_short_data_fields_and_refuses_the_rest() {
        let list =
            |names: &[&str]| validate(&names.iter().map(|n| n.to_string()).collect::<Vec<_>>());
        assert!(list(&["tags", "priority", "legal_status"]).is_ok());
        assert!(list(&[]).is_ok());
        let many: Vec<String> = (0..17).map(|i| format!("f{i}")).collect();
        for bad in [
            validate(&many),
            list(&["tags", "tags"]),
            list(&["Tags"]),
            list(&["1x"]),
            list(&["body"]),
            list(&["title"]),
            list(&[""]),
        ] {
            assert_eq!(bad.unwrap_err().to_string(), REFUSAL);
        }
    }
}
