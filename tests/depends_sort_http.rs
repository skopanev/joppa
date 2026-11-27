//! Black-box HTTP, MCP and Postgres tests for requirement:w17jbz70 and ac:jqc25hpf:
//! ac:e0i44hbz, ac:h7pk3d60, ac:tzshgbmy and ac:jqc25hpf.
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use joppa::{commands::Command as Operation, store};
use read_key::{Auth, call, create_key, read as data_read, session};
use record_world::World;
use serde_json::{Value, json};

async fn mcp_ok(w: &World, token: &str, tool: &str, args: Value) -> Result<Value> {
    w.mcp(token, tool, args)
        .await?
        .map_err(|e| anyhow::anyhow!("{tool}: {e}"))
}

async fn cmd_ok(w: &World, token: &str, command: Value) -> Result<Value> {
    w.command(token, command)
        .await?
        .map_err(|e| anyhow::anyhow!("command: {e}"))
}

async fn http_get(w: &World, query: &str) -> Result<Value> {
    let path = format!("/api/workspaces/{}/objects?{query}", w.ws);
    let (code, body) = call(
        &w.http,
        &w.root,
        "GET",
        &path,
        &[Auth::Bearer(&w.bot)],
        None,
    )
    .await?;
    ensure!(code == 200, "GET {path}: {code} {body}");
    Ok(body)
}

async fn http_err(w: &World, query: &str) -> Result<(u16, String)> {
    let path = format!("/api/workspaces/{}/objects?{query}", w.ws);
    let (code, body) = call(
        &w.http,
        &w.root,
        "GET",
        &path,
        &[Auth::Bearer(&w.bot)],
        None,
    )
    .await?;
    ensure!(code >= 400, "expected error on GET {path}, got 200: {body}");
    Ok((code, body["error"].as_str().unwrap_or_default().to_owned()))
}

fn item_addrs(reply: &Value) -> Vec<String> {
    reply["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["address"].as_str().map(str::to_owned))
        .collect()
}

async fn setup_dep_schema(w: &World) -> Result<()> {
    let tree = mcp_ok(w, &w.alex, "joppa_tree", json!({})).await?;
    let rev = tree["schema"]["revision"].as_u64().context("schema rev")?;
    mcp_ok(
        w,
        &w.alex,
        "joppa_command",
        json!({
            "request_id": uuid::Uuid::new_v4().to_string(),
            "command": {
                "op": "set_tree_schema",
                "revision": rev,
                "reason": "Configure task done rule, priorities, enum, number and computed fields",
                "priorities": ["urgent", "high", "low"],
                "put_types": {
                    "requirement": {
                        "title": "Requirements",
                        "role": "requirement",
                        "status": "children",
                        "root": true,
                        "parents": [],
                        "fields": {
                            "lane": {"type": "enum", "values": ["core", "edge"]}
                        }
                    },
                    "ac": {
                        "title": "Criteria",
                        "role": "ac",
                        "status": "children",
                        "parents": ["requirement"],
                        "fields": {
                            "stage": {"type": "enum", "values": ["to_decide", "to_task", "to_implement", "done"]},
                            "score": {"type": "number"},
                            "lane": {"type": "enum", "values": ["core", "edge"], "from": "parent"}
                        }
                    },
                    "task": {
                        "title": "Task",
                        "role": "task",
                        "status": "none",
                        "parents": ["ac"],
                        "done": {"stage": "done"},
                        "fields": {
                            "stage": {"type": "enum", "values": ["todo", "doing", "done"]},
                            "points": {"type": "number"},
                            "lane": {"type": "enum", "values": ["core", "edge"], "from": "parent"}
                        }
                    },
                    "subtask": {
                        "title": "Subtask",
                        "role": "task",
                        "status": "none",
                        "parents": ["task"],
                        "done": {"stage": "done"},
                        "fields": {
                            "stage": {"type": "enum", "values": ["todo", "done"]}
                        }
                    },
                    "test": {
                        "title": "Tests",
                        "role": "test",
                        "status": "own",
                        "parents": ["ac"],
                        "title_preview_length": 400
                    }
                }
            }
        }),
    )
    .await?;
    Ok(())
}

include!("depends_sort_http/part01.rs");
include!("depends_sort_http/part02.rs");
include!("depends_sort_http/part03.rs");
include!("depends_sort_http/part04.rs");
