//! Small read helpers over a real server (record_world.rs): a GET that keeps a refusal, the
//! MCP tool list, a check for a declared property and the addresses of a page.
#![allow(dead_code)]
use super::record_world::World;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// One GET of `token` below /api/workspaces/<W>. Gives the HTTP code and the body. A body that
/// is not JSON comes back as a JSON string.
pub async fn get_raw(w: &World, token: &str, path: &str) -> Result<(u16, Value)> {
    let url = format!("{}/api/workspaces/{}{path}", w.root, w.ws);
    let reply = w.http.get(url).bearer_auth(token).send().await?;
    let code = reply.status().as_u16();
    let text = reply.text().await?;
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
    Ok((code, body))
}

/// The text of a body: the string itself, or the JSON text.
pub fn text(body: &Value) -> String {
    body.as_str()
        .map_or_else(|| body.to_string(), str::to_owned)
}

/// A GET that the server refuses with an error that contains `error`.
pub async fn get_refused(w: &World, token: &str, path: &str, error: &str) -> Result<()> {
    let (code, body) = get_raw(w, token, path).await?;
    ensure!(
        code >= 400 && text(&body).contains(error),
        "GET {path}: refusal with {error:?} expected, got {code} {body}"
    );
    Ok(())
}

/// The tools of MCP tools/list as `token` reads them.
pub async fn tools(w: &World, token: &str) -> Result<Vec<Value>> {
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
    let reply: Value = (w.http.post(format!("{}/mcp", w.root)))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let tools = reply["result"]["tools"].as_array().cloned();
    tools.with_context(|| format!("tools/list: {reply}"))
}

/// The input property `key` of the tool `name` in a tools/list reply.
pub fn property(tools: &[Value], name: &str, key: &str) -> Value {
    let tool = tools.iter().find(|t| t["name"] == name);
    tool.map_or(Value::Null, |t| t["inputSchema"]["properties"][key].clone())
}

/// True when a "properties" map at any depth of `value` declares `key`.
pub fn declares(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map) => {
            let here = map
                .get("properties")
                .and_then(Value::as_object)
                .is_some_and(|p| p.contains_key(key));
            here || map.values().any(|v| declares(v, key))
        }
        Value::Array(items) => items.iter().any(|v| declares(v, key)),
        _ => false,
    }
}

/// The addresses of the items of a page, in their order.
pub fn addresses(page: &Value) -> Vec<String> {
    let items = page["items"].as_array().into_iter().flatten();
    items
        .filter_map(|i| i["address"].as_str().map(String::from))
        .collect()
}

/// The addresses of the items of a page, sorted.
pub fn sorted(page: &Value) -> Vec<String> {
    let mut all = addresses(page);
    all.sort();
    all
}
