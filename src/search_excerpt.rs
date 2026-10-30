//! The own text of a search result.
use super::*;

/// The data fields that hold the text of an object, in the order that the excerpt prefers.
const TEXT_FIELDS: [&str; 4] = ["body", "description", "contract", "text"];

/// The own text of a result: the first text field of its saved data, cut at 600
/// characters. The title, the other fields and the parents stay out.
pub(super) fn excerpt(data: Option<String>) -> String {
    let data: Value = data
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or(Value::Null);
    let text = TEXT_FIELDS
        .iter()
        .filter_map(|field| data[*field].as_str())
        .find(|text| !text.trim().is_empty())
        .unwrap_or_default();
    text.chars().take(600).collect()
}
