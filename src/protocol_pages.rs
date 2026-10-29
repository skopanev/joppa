//! One page shape for the MCP list reads (requirement:read-contract): at most 25 items,
//! `{items, total, next_cursor}`, and `cursor` for the next page. A larger `limit` is
//! clamped to 25, never refused. The HTTP routes keep their own page sizes.
use anyhow::{Context, ensure};
use deadpool_postgres::Pool;
use joppa::{read_compact as rc, store};
use serde_json::{Value, json};

/// The page size of a list, from `limit`: `default` without it, at most rc::PAGE.
pub fn limit(a: &Value, default: usize) -> anyhow::Result<usize> {
    let Some(value) = a.get("limit").filter(|v| !v.is_null()) else {
        return Ok(default);
    };
    let n = value.as_u64().context("limit must be a whole number")?;
    ensure!(n >= 1, "limit must be at least 1");
    Ok((n as usize).min(rc::PAGE))
}

/// joppa_list: the workspaces that the actor can read, in pages of 25.
pub async fn list(pool: &Pool, actor: &str, a: &Value) -> anyhow::Result<Value> {
    let listed = store::list(pool, actor).await?;
    let items = listed.as_array().cloned().unwrap_or_default();
    rc::page(items, a["cursor"].as_str())
}

/// joppa_search: one page of the ranked results, 10 by default. A hit gives no public ID,
/// no `req` on a requirement, and its `workspace` only when the search reads several.
pub async fn search(
    pool: &Pool,
    actor: &str,
    a: &Value,
    embedder: Option<&joppa::embeddings::Embedder>,
) -> anyhow::Result<Value> {
    let text = |k: &str| a[k].as_str().with_context(|| format!("missing {k}"));
    let kind = match a.get("kind") {
        Some(v) => Some(v.as_str().context("kind must be a string")?),
        None => None,
    };
    let flag = |name: &str| match a.get(name) {
        Some(v) => v
            .as_bool()
            .with_context(|| format!("{name} must be a boolean")),
        None => Ok(false),
    };
    let offset = match a["cursor"].as_str() {
        Some(c) => c.parse().map_err(|_| anyhow::anyhow!("invalid cursor"))?,
        None => 0,
    };
    let flags = store::search::Flags {
        all_versions: flag("all_versions")?,
        all_workspaces: flag("all_workspaces")?,
        offset,
    };
    let mut reply = store::search::find_with(
        pool,
        actor,
        text("workspace")?,
        text("query")?,
        kind,
        limit(a, 10)? as i64,
        embedder,
        flags,
    )
    .await?;
    for item in reply["items"].as_array_mut().into_iter().flatten() {
        let Some(map) = item.as_object_mut() else {
            continue;
        };
        map.remove("system_id");
        if map.get("kind").and_then(Value::as_str) == Some("requirement") {
            map.remove("req");
        }
        if !flags.all_workspaces {
            map.remove("workspace");
        }
    }
    Ok(reply)
}

/// joppa_tasks: the Task queue with its tasks as `items`.
pub fn tasks(mut reply: Value) -> Value {
    if let Some(map) = reply.as_object_mut()
        && let Some(tasks) = map.remove("tasks")
    {
        map.insert("items".into(), tasks);
    }
    reply
}

/// joppa_history: the History read with a clamped page size, 25 by default.
pub fn history_args(a: &Value) -> anyhow::Result<Value> {
    let mut args = a.clone();
    args["limit"] = json!(limit(a, rc::PAGE)?);
    Ok(args)
}
