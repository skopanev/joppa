//! Black-box HTTP, MCP, search and /api/data tests for ac:zk5119jk and ac:ow9ghazl.
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use joppa::store;
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

fn enc(value: &Value) -> String {
    let raw = value.to_string();
    let mut out = String::with_capacity(raw.len() * 3);
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn http_get_path(w: &World, route: &str, query: &str) -> Result<Value> {
    let path = format!("/api/workspaces/{}/{route}?{query}", w.ws);
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
        .filter_map(|i| {
            i["address"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| Some(format!("{}:{}", i["kind"].as_str()?, i["id"].as_str()?)))
        })
        .collect()
}

async fn setup_selector_schema(w: &World) -> Result<()> {
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
                "reason": "Configure selector schema with tags, nested delivery, computed lane and task done rule",
                "put_types": {
                    "domain": {
                        "title": "Domain",
                        "role": "group",
                        "status": "children",
                        "root": true,
                        "parents": [],
                        "fields": {
                            "lane": {"type": "enum", "values": ["payments", "identity"]}
                        }
                    },
                    "requirement": {
                        "title": "Requirement",
                        "role": "requirement",
                        "status": "children",
                        "root": true,
                        "parents": ["domain"],
                        "fields": {
                            "lane": {"type": "enum", "values": ["payments", "identity"], "from": "parent"}
                        }
                    },
                    "ac": {
                        "title": "Criterion",
                        "role": "ac",
                        "status": "children",
                        "parents": ["requirement"],
                        "fields": {
                            "lane": {"type": "enum", "values": ["payments", "identity"], "from": "parent"}
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
                            "tags": {"type": "list"},
                            "lane": {"type": "enum", "values": ["payments", "identity"], "from": "parent"}
                        }
                    },
                    "test": {
                        "title": "Test",
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

include!("native_tiers_http/part01.rs");
include!("native_tiers_http/part02.rs");
