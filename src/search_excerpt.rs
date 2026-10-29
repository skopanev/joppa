//! The own text of a search result.
use super::*;

/// The own text of a result: its saved data without the title, which the result already
/// gives, and without empty values, cut at 600 characters. The parents stay out.
pub(super) fn excerpt(data: Option<String>) -> String {
    let mut data: Value = data
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or(Value::Null);
    if let Some(map) = data.as_object_mut() {
        map.remove("title");
        map.retain(|_, v| {
            !matches!(v, Value::Null | Value::Bool(false))
                && v != &json!("")
                && v != &json!([])
                && v != &json!({})
        });
    }
    let text = if data.is_null() {
        String::new()
    } else {
        data.to_string()
    };
    text.chars().take(600).collect()
}
