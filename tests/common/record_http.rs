//! Record test outcomes over a real server with the MCP tool joppa_record_results.
//! Tests that need results call this helper; it is the only way to write them.
#![allow(dead_code)]
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// Record `results` for `commit` in workspace `ws` with `token`.
/// `root` is the server address, such as http://127.0.0.1:PORT.
/// Gives Ok(reply) for a stored record and Err(text) for a refusal.
pub async fn record(
    http: &reqwest::Client,
    root: &str,
    ws: &str,
    token: &str,
    commit: &str,
    results: Value,
) -> Result<Result<Value, String>> {
    let args = json!({"workspace":ws,"commit":commit,"results":results});
    let reply = http
        .post(format!("{root}/mcp"))
        .bearer_auth(token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_record_results","arguments":args}}))
        .send()
        .await?;
    let code = reply.status().as_u16();
    let body: Value = reply.json().await?;
    ensure!(code == 200, "joppa_record_results: {code} {body}");
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .with_context(|| format!("MCP reply: {body}"))?;
    Ok(if body["result"]["isError"] == true {
        Err(text.to_owned())
    } else {
        Ok(serde_json::from_str(text)?)
    })
}

/// A record that must succeed.
pub async fn recorded(
    http: &reqwest::Client,
    root: &str,
    ws: &str,
    token: &str,
    commit: &str,
    results: Value,
) -> Result<Value> {
    record(http, root, ws, token, commit, results)
        .await?
        .map_err(|e| anyhow::anyhow!("record {commit}: {e}"))
}

/// Results for the cases of one test file: each case is (name, "pass" | "fail" | "skip").
/// A failure has the message "boom".
pub fn cases(prefix: &str, cases: &[(&str, &str)]) -> Value {
    cases
        .iter()
        .map(|(name, status)| match *status {
            "fail" => json!({"test":format!("{prefix} > {name}"),"status":"fail","message":"boom"}),
            other => json!({"test":format!("{prefix} > {name}"),"status":other}),
        })
        .collect()
}
