//! The compact MCP reply that matches one HTTP reply: MCP reads use the same store reads
//! and then the shape rules of requirement:read-contract.
use serde_json::Value;

/// The expected MCP reply for `http`.
pub fn compact_of(http: &Value, mcp: &Value) -> Value {
    use joppa::read_compact as rc;
    if http.get("items").is_some() && http.get("address").is_none() {
        let mut http = http.clone();
        if let Some(cursor) = mcp.get("next_cursor") {
            http["next_cursor"] = cursor.clone();
        }
        let mut list = rc::list(http, None);
        if let Some(total) = mcp.get("total") {
            list["total"] = total.clone();
        }
        list
    } else if http.get("document").is_some() && mcp.get("historical").is_none() {
        rc::detail(http.clone(), None, None).unwrap_or(Value::Null)
    } else if http.get("document").is_some() || http.get("revisions").is_some() {
        // A revision read or the revision headers.
        rc::revision(http.clone())
    } else {
        rc::strip_reply(http.clone())
    }
}
