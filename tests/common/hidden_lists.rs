//! The reads and writes of the per-member hidden flag over a real record world: the web lists
//! of one token, the joppa_list item of one token, the commands, a second workspace and a
//! rebuild of the world workspace at server start.
#![allow(dead_code)]
use crate::record_world::World;
use anyhow::{Context, Result, ensure};
use joppa::{commands::Command as Operation, store};
use serde_json::{Value, json};

/// A GET of the server with `token`: the HTTP code and the body.
pub async fn get(w: &World, token: &str, path: &str) -> Result<(u16, Value)> {
    let url = format!("{}{path}", w.root);
    let reply = w.http.get(url).bearer_auth(token).send().await?;
    Ok((reply.status().as_u16(), reply.json().await?))
}

/// The items of a web list: GET /api/workspaces, or the items of GET /api/access.
pub async fn items(w: &World, token: &str, path: &str) -> Result<Vec<Value>> {
    let (code, body) = get(w, token, path).await?;
    ensure!(code == 200, "{path}: {code} {body}");
    let items = body.get("items").unwrap_or(&body).as_array().cloned();
    items.with_context(|| format!("{path}: {body}"))
}

/// The workspace IDs of a web list.
pub async fn web(w: &World, token: &str, path: &str) -> Result<Vec<String>> {
    Ok(items(w, token, path)
        .await?
        .iter()
        .filter_map(|i| i["id"].as_str().map(String::from))
        .collect())
}

/// Ok when a joppa_list item is {id, title}, or {id, title, hidden: true} for a workspace that
/// the caller hides (tac:tcospnl8).
pub fn lean_item(item: &Value) -> Result<()> {
    let map = item.as_object().with_context(|| format!("item {item}"))?;
    let text = |k: &str| map.get(k).is_some_and(Value::is_string);
    let named = text("id") && text("title");
    let keys = map
        .iter()
        .all(|(k, v)| k == "id" || k == "title" || (k == "hidden" && *v == json!(true)));
    ensure!(
        named && keys,
        "a joppa_list item is not {{id, title, hidden?}}: {item}"
    );
    Ok(())
}

/// Every joppa_list item of the member of `token`, through all pages. Each item passes
/// `lean_item`.
pub async fn joppa_list(w: &World, token: &str) -> Result<Vec<Value>> {
    let (mut all, mut cursor) = (vec![], Value::Null);
    loop {
        let args = if cursor.is_null() {
            json!({})
        } else {
            json!({ "cursor": cursor })
        };
        let page = w.mcp(token, "joppa_list", args).await?;
        let page = page.map_err(anyhow::Error::msg)?;
        let items = page["items"].as_array().cloned().unwrap_or_default();
        items.iter().try_for_each(lean_item)?;
        all.extend(items);
        cursor = page["next_cursor"].clone();
        if !cursor.is_string() {
            return Ok(all);
        }
    }
}

/// The joppa_list item of workspace `ws` for the member of `token`.
pub async fn listed_in(w: &World, token: &str, ws: &str) -> Result<Value> {
    let items = joppa_list(w, token).await?;
    let item = items.into_iter().find(|i| i["id"] == ws);
    item.with_context(|| format!("joppa_list item of {ws}"))
}

/// The joppa_list item of the world workspace for the member of `token`.
pub async fn listed(w: &World, token: &str) -> Result<Value> {
    listed_in(w, token, &w.ws).await
}

/// A command that must succeed.
pub async fn ok(w: &World, token: &str, command: Value) -> Result<Value> {
    let reply = w.command(token, command.clone()).await?;
    reply.map_err(|e| anyhow::anyhow!("{command}: {e}"))
}

/// The refusal of one command, or an error when the command succeeds.
pub async fn refused(w: &World, token: &str, command: Value) -> Result<String> {
    match w.command(token, command.clone()).await? {
        Err(text) => Ok(text),
        Ok(reply) => anyhow::bail!("{command} was accepted: {reply}"),
    }
}

/// update_workspace with only `hidden`.
pub fn hide(hidden: bool) -> Value {
    json!({"op":"update_workspace","hidden":hidden})
}

/// A second workspace of the owner with the member alex. Gives its ID.
pub async fn second_workspace(w: &World) -> Result<String> {
    let ws = format!("record-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Second channel"}),
        json!({"op":"member","id":"alex","role":"human"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &ws, "owner", &format!("second-{i}"), &op).await?;
    }
    Ok(ws)
}

/// Stop the server, run `sql` on the board projection of the world workspace ($1), and start
/// the server again. The server rebuilds a stale projection from the journal at start.
pub async fn restart_after(w: &mut World, sql: &str) -> Result<()> {
    w.stop();
    let rows = w.pool.get().await?.execute(sql, &[&w.ws]).await?;
    ensure!(rows == 1, "board projection of {}", w.ws);
    w.start().await
}

/// Make the server rebuild the world workspace from its journal at start.
pub async fn rebuild(w: &mut World) -> Result<()> {
    let sql = "UPDATE joppa.board_cache SET first_view='{}'::jsonb WHERE workspace=$1";
    restart_after(w, sql).await
}
