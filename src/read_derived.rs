//! Part of `read_compact`: the fields that a read derives from an object item, moved from
//! read_fields.rs.
use serde_json::{Value, json};

/// The names of the derived fields.
pub const DERIVED: [&str; 3] = ["excerpt", "data_keys", "status_label"];

/// The fields that a read derives from an item: the first 200 characters of the body with
/// single spaces, the sorted keys of the data and the computed values, and the status label.
pub fn derived(item: &Value, field: &str) -> Option<Value> {
    Some(match field {
        "excerpt" => {
            let body = item["body"].as_str().unwrap_or_default();
            let words: Vec<&str> = body.split_whitespace().collect();
            let text: String = words.join(" ").chars().take(200).collect();
            if text.is_empty() {
                Value::Null
            } else {
                json!(text)
            }
        }
        "data_keys" => {
            let maps = [&item["data"], &item["computed"]];
            let keys = maps.into_iter().filter_map(Value::as_object).flatten();
            let keys = keys.filter(|(_, v)| !v.is_null()).map(|(k, _)| k.as_str());
            json!(keys.collect::<std::collections::BTreeSet<_>>())
        }
        "status_label" => match item["signal"]["label"].as_str() {
            Some(label) if !label.is_empty() => json!(label),
            _ => Value::Null,
        },
        _ => return None,
    })
}

/// The parent hash facts of an item (parent_hash.rs): its own contract hash, the stamp of its
/// revision, the current contract hash of its parent, and whether a stamp differs from it.
pub fn parent_hash_facts(
    item: &mut Value,
    contract: Option<String>,
    (stamp, current): (Option<String>, Option<String>),
) {
    item["contract_hash"] = json!(contract);
    item["parent_changed"] = json!(stamp.is_some() && stamp != current);
    item["parent_hash"] = json!(stamp);
    item["parent_hash_current"] = json!(current);
}

/// The refusal of a read of an unknown address. An address whose type the workspace lacks
/// names the valid types.
pub fn unknown_object(asked: &str, types: &[String]) -> String {
    let mut valid: Vec<&str> = types.iter().map(String::as_str).collect();
    valid.sort_unstable();
    match asked.split_once(':') {
        Some((kind, _)) if !valid.contains(&kind) => format!(
            "unknown object: {asked} (unknown type {kind}, valid types are {})",
            valid.join(", ")
        ),
        _ => format!("unknown object: {asked}"),
    }
}

#[cfg(test)]
#[path = "read_derived_tests.rs"]
mod tests;
