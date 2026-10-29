//! The compact MCP reply that matches one HTTP reply: MCP reads use the same store reads
//! and then the shape rules of requirement:read-contract.
use serde_json::Value;

/// The expected MCP reply for `http`.
pub fn compact_of(http: &Value, mcp: &Value) -> Value {
    use joppa::read_compact as rc;
    if http.get("tasks").is_some() {
        // The Task queue: MCP gives the tasks as `items`.
        let mut http = http.clone();
        if let Some(map) = http.as_object_mut()
            && let Some(tasks) = map.remove("tasks")
        {
            map.insert("items".into(), tasks);
        }
        rc::strip_reply(http)
    } else if http.get("items").is_some() && http.get("address").is_none() {
        let mut http = http.clone();
        if let Some(cursor) = mcp.get("next_cursor") {
            http["next_cursor"] = cursor.clone();
        }
        let mut list = rc::list(http, None);
        if let Some(total) = mcp.get("total") {
            list["total"] = total.clone();
        }
        list
    } else if http.get("revisions").is_some() {
        // The revision headers: the first page of 25 as `items`, with the total.
        let mut http = http.clone();
        let revisions = http["revisions"]
            .take()
            .as_array()
            .cloned()
            .unwrap_or_default();
        http.as_object_mut().map(|m| m.remove("revisions"));
        let page = rc::page(revisions, None).unwrap_or_default();
        let mut out = rc::revision(http);
        for key in ["items", "total", "next_cursor"] {
            if !page[key].is_null() {
                out[key] = page[key].clone();
            }
        }
        out
    } else if http.get("document").is_some() && mcp.get("historical").is_none() {
        rc::detail(http.clone(), None, None).unwrap_or(Value::Null)
    } else if http.get("document").is_some() {
        // A revision read.
        rc::revision(http.clone())
    } else {
        rc::strip_reply(http.clone())
    }
}
