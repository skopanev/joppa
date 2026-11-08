//! The HTTP History page as MCP gives it.
use serde_json::{Value, json};

/// No empty keys, no echo of the request, no signal, and no workspace or snapshot position.
pub fn history(http: &Value) -> Value {
    let out = joppa::read_compact::strip_reply(joppa::mcp_shapes::history(http.clone()));
    joppa::mcp_lean::lean("joppa_history", &json!({}), out)
}
