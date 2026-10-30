//! The HTTP timeline as MCP gives it.
use serde_json::{Value, json};

/// No empty keys, no echo of after and order, and no workspace or snapshot position.
pub fn timeline(http: &Value) -> Value {
    let mut out = joppa::read_compact::strip_reply(http.clone());
    out.as_object_mut()
        .map(|m| (m.remove("after"), m.remove("order")));
    joppa::mcp_lean::lean("joppa_timeline", &json!({}), out)
}
