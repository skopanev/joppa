//! The compact MCP reply that matches one HTTP reply: MCP reads use the same store reads
//! and then the shape rules of requirement:read-contract.
use serde_json::Value;

/// The tool that gives the MCP copy of `http`.
fn tool_of(http: &Value) -> &'static str {
    let first = &http["items"][0];
    if http.get("tasks").is_some() {
        "joppa_tasks"
    } else if http.get("schema").is_some() {
        "joppa_tree"
    } else if http.get("semantic_status").is_some() || http.get("mode").is_some() {
        "joppa_search"
    } else if http.get("tests").is_some() || first.get("tests").is_some() {
        "joppa_tests"
    } else {
        "joppa_objects"
    }
}

/// The expected MCP reply for `http`: the shape rules, then the lean rules of the tool.
pub fn compact_of(http: &Value, mcp: &Value) -> Value {
    let asked = http["address"].as_str().unwrap_or_default();
    let args = serde_json::json!({"address":asked,"ac":asked,"workspace":http["workspace"]});
    joppa::mcp_lean::lean(tool_of(http), &args, shape(http, mcp, asked))
}

fn shape(http: &Value, mcp: &Value, asked: &str) -> Value {
    use joppa::read_compact as rc;
    if http.get("tasks").is_some() {
        // The Task queue: MCP gives the tasks as `items`.
        let mut http = http.clone();
        if let Some(map) = http.as_object_mut()
            && let Some(tasks) = map.remove("tasks")
        {
            map.insert("items".into(), tasks);
        }
        rc::strip_reply(joppa::mcp_shapes::tasks(http))
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
        let mut out = rc::headers(http, asked);
        for key in ["items", "total", "next_cursor"] {
            if !page[key].is_null() {
                out[key] = page[key].clone();
            }
        }
        out
    } else if http.get("document").is_some() && mcp.get("current_revision").is_none() {
        // A detail read gives no current_revision; a revision read gives it.
        rc::detail(http.clone(), asked, None).unwrap_or(Value::Null)
    } else if http.get("document").is_some() {
        // A revision read.
        rc::revision(http.clone(), asked)
    } else {
        rc::strip_reply(http.clone())
    }
}
