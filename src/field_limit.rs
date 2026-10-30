//! The length limit of object fields. Each new or changed scalar in the data of an object
//! holds at most the `field_max_length` of its type: 1 to FIELD_CEILING characters, and
//! FIELD_MAX without a value. A save may keep a stored value of any length, so an older long
//! text stays editable in its other fields. Migration 037 applies FIELD_CEILING in the database.
use anyhow::{Result, bail};
use serde_json::Value;

/// The limit of a type without `field_max_length`.
pub const FIELD_MAX: usize = 1024;
/// The largest `field_max_length` of a type: 256 KiB of characters.
pub const FIELD_CEILING: usize = 262144;
/// The advice next to each field limit, in the words of the owner.
pub const HINT: &str = "Over 1 kb: prefer attach as a file";

/// The limit of a type with the `field_max_length` value `length`.
pub fn limit(length: Option<u32>) -> usize {
    length.map_or(FIELD_MAX, |n| n as usize)
}

/// The path of the first new or changed scalar of `new` that is longer than `limit`.
/// `old` is the stored data at the same place, if any.
pub fn too_long(new: &Value, old: Option<&Value>, limit: usize) -> Option<String> {
    walk(new, old, limit, &mut vec![])
}

fn walk(new: &Value, old: Option<&Value>, limit: usize, path: &mut Vec<String>) -> Option<String> {
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
            return (text.chars().count() > limit && old != Some(new)).then(|| path.join("."));
        }
        other => {
            return (other.to_string().chars().count() > limit && old != Some(new))
                .then(|| path.join("."));
        }
    };
    for (key, value, before) in children {
        path.push(key);
        let found = walk(value, before, limit, path);
        path.pop();
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Refuse new data that holds a new or changed value longer than `limit`.
pub fn check(new: &Value, old: Option<&Value>, limit: usize) -> Result<()> {
    if let Some(path) = too_long(new, old, limit) {
        bail!(
            "field data.{path} is at most {limit} characters; split the text into child objects or attach a file. {HINT}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_new_or_changed_value_over_the_limit_is_refused() {
        let long = "я".repeat(FIELD_MAX + 1);
        let at_limit = "я".repeat(FIELD_MAX);
        assert!(check(&json!({"description": at_limit}), None, FIELD_MAX).is_ok());
        let found = |data: Value| too_long(&data, None, FIELD_MAX);
        assert_eq!(
            found(json!({"description": long})).as_deref(),
            Some("description")
        );
        assert_eq!(
            found(json!({"tests": ["short", long]})).as_deref(),
            Some("tests.1")
        );
        assert_eq!(
            found(json!({"meta": {"note": long}})).as_deref(),
            Some("meta.note")
        );
        let changed = json!({"body": format!("{long}x")});
        assert!(too_long(&changed, Some(&json!({"body": long})), FIELD_MAX).is_some());
        // A type limit replaces the default, up to the ceiling.
        assert!(check(&json!({"body": long}), None, limit(Some(4096))).is_ok());
        assert!(too_long(&json!({"body": "x".repeat(4097)}), None, 4096).is_some());
        assert_eq!(limit(None), 1024);
    }

    #[test]
    fn a_stored_long_value_stays_when_another_field_changes() {
        let long = "x".repeat(FIELD_MAX + 500);
        let old = json!({"title": "Old", "body": long, "status": "unprocessed"});
        let new = json!({"title": "New", "body": long, "status": "processed"});
        assert!(check(&new, Some(&old), FIELD_MAX).is_ok());
    }
}
