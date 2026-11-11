//! One command through POST /api/workspaces/{w}/commands of a record world. MCP hides the
//! admin operations (attach, member, remove_member, update_workspace and the others), and this
//! route keeps them.
#![allow(dead_code)]
use crate::record_world::World;
use anyhow::Result;
use serde_json::{Value, json};

/// One command of `token` in the world workspace. A refusal gives Err with its error text.
pub async fn command(w: &World, token: &str, command: Value) -> Result<Result<Value, String>> {
    let id = uuid::Uuid::new_v4().to_string();
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(token)
        .json(&json!({"request_id":id,"command":command}))
        .send()
        .await?;
    let accepted = reply.status().is_success();
    let text = reply.text().await?;
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok(if accepted {
        Ok(body)
    } else {
        Err(body["error"].as_str().map_or(text, String::from))
    })
}

/// A command that must succeed. Gives the reply.
pub async fn ok(w: &World, token: &str, command: Value) -> Result<Value> {
    let reply = self::command(w, token, command.clone()).await?;
    reply.map_err(|e| anyhow::anyhow!("HTTP {command}: {e}"))
}

/// The refusal text of a command, or an error when the command succeeds.
pub async fn refused(w: &World, token: &str, command: Value) -> Result<String> {
    match self::command(w, token, command.clone()).await? {
        Err(text) => Ok(text),
        Ok(reply) => anyhow::bail!("HTTP {command} was accepted: {reply}"),
    }
}
