//! The HTTP timeline as MCP gives it.
use serde_json::Value;

/// No empty keys and no echo of after and order.
pub fn timeline(http: &Value) -> Value {
    let mut out = joppa::read_compact::strip_reply(http.clone());
    out.as_object_mut()
        .map(|m| (m.remove("after"), m.remove("order")));
    out
}
