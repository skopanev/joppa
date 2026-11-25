//! Black-box HTTP, MCP and Postgres tests for requirement:4ea08btm:
//! ac:dvur104e, ac:ipdef4dm, ac:fug439p5 and ac:1pbb9gtx.
#[path = "common/manual_check.rs"]
mod manual_check;
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/save_world.rs"]
mod save_world;
use anyhow::{Context, Result, ensure};
use read_key::{Auth, call, session};
use record_world::World;
use save_world::{head, ok, refused, replay, revisions, save};
use serde_json::{Value, json};

async fn mcp_ok(w: &World, token: &str, tool: &str, args: Value) -> Result<Value> {
    w.mcp(token, tool, args)
        .await?
        .map_err(|e| anyhow::anyhow!("{tool}: {e}"))
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

fn is_sha256_hex(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

async fn create_write_key(w: &World) -> Result<String> {
    let cookie = session(w, "owner").await?;
    let auth = [Auth::Cookie(&cookie), Auth::Origin(&w.root)];
    let path = format!("/api/workspaces/{}/write-key", w.ws);
    let (code, body) = call(&w.http, &w.root, "POST", &path, &auth, None).await?;
    ensure!(code == 200, "POST write-key: {code} {body}");
    body["key"].as_str().map(str::to_owned).context("write key")
}

async fn setup_domain_capability(w: &World, key_write_stage: bool) -> Result<()> {
    let tree = mcp_ok(w, &w.bot, "joppa_tree", json!({})).await?;
    let fields = if key_write_stage {
        json!({"stage":{"type":"enum","values":["todo","done"],"key_write":true}})
    } else {
        json!({})
    };
    w.command(
        &w.owner,
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "put_types": {
                "domain": {"title":"Domain","role":"group","root":true,"status":"children","parents":[]},
                "capability": {"title":"Capability","role":"group","root":false,"status":"children","parents":["domain"],"fields":fields}
            },
            "reason": "Add domain and capability types"
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    Ok(())
}

include!("parent_hash_http/part01.rs");
include!("parent_hash_http/part02.rs");
