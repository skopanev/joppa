//! The length limit of object fields. Each new or changed scalar in the data of an object
//! holds at most FIELD_MAX characters. A save may keep a stored value of any length, so an
//! older long text stays editable in its other fields. Migration 037 applies the same rule
//! in the database.
use anyhow::{Result, bail};
use serde_json::Value;

pub const FIELD_MAX: usize = 1024;

/// The path of the first new or changed scalar of `new` that is longer than FIELD_MAX.
/// `old` is the stored data at the same place, if any.
pub fn too_long(new: &Value, old: Option<&Value>) -> Option<String> {
    walk(new, old, &mut vec![])
}

fn walk(new: &Value, old: Option<&Value>, path: &mut Vec<String>) -> Option<String> {
    let children: Vec<(String, &Value, Option<&Value>)> = match new {
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| (k.clone(), v, old.and_then(|o| o.get(k))))
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v, old.and_then(|o| o.get(i))))
            .collect(),
        Value::Null => return None,
        Value::String(text) => {
            return (text.chars().count() > FIELD_MAX && old != Some(new)).then(|| path.join("."));
        }
        other => {
            return (other.to_string().chars().count() > FIELD_MAX && old != Some(new))
                .then(|| path.join("."));
        }
    };
    for (key, value, before) in children {
        path.push(key);
        let found = walk(value, before, path);
        path.pop();
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Refuse new data that holds a new or changed value longer than FIELD_MAX.
pub fn check(new: &Value, old: Option<&Value>) -> Result<()> {
    if let Some(path) = too_long(new, old) {
        bail!(
            "field data.{path} is at most {FIELD_MAX} characters; split the text into child objects or attach a file"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_new_or_changed_value_over_1024_characters_is_refused() {
        let long = "я".repeat(FIELD_MAX + 1);
        let limit = "я".repeat(FIELD_MAX);
        assert!(check(&json!({"description": limit}), None).is_ok());
        assert_eq!(
            too_long(&json!({"description": long}), None).as_deref(),
            Some("description")
        );
        assert_eq!(
            too_long(&json!({"tests": ["short", long]}), None).as_deref(),
            Some("tests.1")
        );
        assert_eq!(
            too_long(&json!({"meta": {"note": long}}), None).as_deref(),
            Some("meta.note")
        );
        let changed = json!({"body": format!("{long}x")});
        assert!(too_long(&changed, Some(&json!({"body": long}))).is_some());
    }

    #[test]
    fn a_stored_long_value_stays_when_another_field_changes() {
        let long = "x".repeat(FIELD_MAX + 500);
        let old = json!({"title": "Old", "body": long, "status": "unprocessed"});
        let new = json!({"title": "New", "body": long, "status": "processed"});
        assert!(check(&new, Some(&old)).is_ok());
    }
}
