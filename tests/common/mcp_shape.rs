//! The HTTP History page as MCP gives it (ac:jkvdy2jl): no empty keys, no echo of the request,
//! no workspace or snapshot position, and an entry without signal, kind and a change equal to
//! its op. An MCP `at` has whole seconds.
#![allow(dead_code)]
use serde_json::{Value, json};

/// The digits of a timestamp up to whole seconds, YYYYMMDDHHMMSS, as a string. Both forms
/// "2026-01-01 10:00:00.123+00" and "2026-01-01T10:00:00Z" give "20260101100000".
pub fn seconds(at: &Value) -> Value {
    let digits: String = at
        .as_str()
        .unwrap_or_default()
        .chars()
        .filter(char::is_ascii_digit)
        .take(14)
        .collect();
    json!(digits)
}

/// The MCP page that the HTTP page `http` gives, with each `at` as `seconds`.
pub fn history(http: &Value) -> Value {
    let mut out = joppa::read_compact::strip_reply(http.clone());
    if let Some(map) = out.as_object_mut() {
        for key in ["open", "order", "after", "limit", "workspace", "position"] {
            map.remove(key);
        }
    }
    for item in out["items"].as_array_mut().into_iter().flatten() {
        let at = seconds(&item["at"]);
        let Some(map) = item.as_object_mut() else {
            continue;
        };
        map.remove("signal");
        map.remove("kind");
        if map.get("change").is_some() && map.get("change") == map.get("op") {
            map.remove("change");
        }
        map.insert("at".into(), at);
        processed_seconds(item);
    }
    out
}

/// The `at` of the mark of a processed entry as `seconds`.
fn processed_seconds(item: &mut Value) {
    if item["processed"]["at"].is_string() {
        item["processed"]["at"] = seconds(&item["processed"]["at"]);
    }
}

/// The MCP page `mcp` with each `at` as `seconds`. An `at` with a fraction of a second gives
/// Err.
pub fn whole_seconds(mcp: &Value) -> Result<Value, String> {
    let mut out = mcp.clone();
    for item in out["items"].as_array_mut().into_iter().flatten() {
        let at = item["at"].as_str().unwrap_or_default();
        if at.contains('.') || at.len() < 19 {
            return Err(format!("an MCP at without whole seconds: {item}"));
        }
        item["at"] = seconds(&item["at"]);
        processed_seconds(item);
    }
    Ok(out)
}

/// Ok when the MCP page `mcp` is the HTTP page `http` as MCP gives it.
pub fn same_history(http: &Value, mcp: &Value) -> Result<(), String> {
    let found = whole_seconds(mcp)?;
    let wanted = history(http);
    if found == wanted {
        Ok(())
    } else {
        Err(format!("HTTP {http}\nMCP {mcp}\nwanted {wanted}"))
    }
}
