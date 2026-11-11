//! The initialize instructions and tools/list stay small, and each parameter describes itself.
//! The sizes count the strings as POST /mcp sends them.
use serde_json::{Value, json};

/// The raw body of the tools/list reply, as mcp_http sends it.
fn tools_body() -> String {
    json!({"jsonrpc":"2.0","id":1,"result":{"tools":super::tools::list()}}).to_string()
}

/// Each `properties` entry under `value`, with its path.
fn properties<'a>(value: &'a Value, path: &str, found: &mut Vec<(String, &'a Value)>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let here = format!("{path}.{key}");
                if key == "properties" {
                    for (name, property) in child.as_object().into_iter().flatten() {
                        found.push((format!("{here}.{name}"), property));
                    }
                }
                properties(child, &here, found);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| properties(v, path, found)),
        _ => {}
    }
}

#[test]
fn initialize_gives_the_read_and_write_rules_in_at_most_3500_bytes() {
    let text = super::INSTRUCTIONS;
    assert!(text.len() <= 3500, "{} bytes", text.len());
    for word in [
        "next_cursor",
        "fields",
        "2 to 25 addresses",
        "revision",
        "relations",
        "Evidence",
        "alias",
        "Over 1 kb: prefer attach as a file",
        "existing OAuth login",
        "reads a public item ID",
    ] {
        assert!(text.contains(word), "no {word:?}");
    }
    for word in [
        "11111",
        "Reviewer",
        "Test-5",
        "ASD-STE100 Issue 9",
        "Resolve a copied public ID",
        "joppa_search({",
    ] {
        assert!(!text.contains(word), "{word:?}");
    }
}

/// The budget of tools/list. The reply had 19603 bytes before stage 2 of the payload diet.
const TOOLS_BUDGET: usize = 13500;

#[test]
fn tools_list_stays_in_its_budget_and_each_property_has_a_short_description() {
    let body = tools_body();
    assert!(
        body.len() <= TOOLS_BUDGET,
        "tools/list has {} bytes",
        body.len()
    );
    for (text, most) in [
        ("Optional. Your agent name under this account", 0),
        ("Over 1 kb: prefer attach as a file", 1),
        ("in at least 80% of sentences", 1),
        ("field_hint", 0),
    ] {
        let count = body.matches(text).count();
        assert!(count <= most, "{text:?} occurs {count} times");
    }
    let reply: Value = serde_json::from_str(&body).unwrap();
    let mut found = vec![];
    properties(&reply, "", &mut found);
    for (path, property) in &found {
        let text = property["description"].as_str().unwrap_or_default();
        assert!(!text.trim().is_empty(), "{path} has no description");
        if path.ends_with(".alias") {
            assert!(text.chars().count() <= 40, "{path}: {text}");
        }
    }
    let tools = reply["result"]["tools"].as_array().unwrap();
    let tool = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap();
    let fields = &tool("joppa_objects")["inputSchema"]["properties"]["fields"]["description"];
    let fields = fields.as_str().unwrap();
    assert!(
        fields.contains("on a list and on an address read"),
        "{fields}"
    );
    let help = &tool("joppa_help")["inputSchema"]["properties"]["section"]["enum"];
    assert!(!help.as_array().unwrap().contains(&json!("tree")), "{help}");
    let command = &tool("joppa_command")["inputSchema"]["properties"]["command"];
    let ops: Vec<&str> = joppa::commands::mcp_operations().map(|o| o.name).collect();
    assert_eq!(command["properties"]["op"]["enum"], json!(ops));
}
