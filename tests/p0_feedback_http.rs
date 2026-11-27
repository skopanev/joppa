//! Black-box HTTP, MCP and Postgres tests for the five P0 agent feedback fixes:
//! ac:9vb5eaq3, ac:3qhus6yb, ac:izklhyhv, ac:1w9zqzg9 and ac:wsycpf5i.
#[path = "common/manual_check.rs"]
mod manual_check;
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/save_world.rs"]
mod save_world;
use anyhow::{Context, Result, ensure};
use read_key::{Auth, call, create_key, data, read, session};
use record_world::World;
use save_world::{head, ok, refused, save};
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

async fn setup_p0_schema(w: &World) -> Result<()> {
    let tree = mcp_ok(w, &w.bot, "joppa_tree", json!({})).await?;
    w.command(
        &w.owner,
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "put_types": {
                "domain": {"title":"Domain","role":"group","root":true,"status":"children","parents":[]},
                "capability": {"title":"Capability","role":"group","root":false,"status":"children","parents":["domain"]},
                "source": {"title":"Source","role":"document","root":true,"source":true,"status":"none","parents":[]},
                "note": {"title":"Note","role":"document","root":true,"inbox":true,"status":"none","parents":[]},
                "ac": {
                    "title":"AC","role":"ac","root":false,"status":"own","parents":["requirement","capability"],
                    "references":{"sources":{"title":"Sources","target":"source","multiple":true}}
                }
            },
            "reason": "Configure schema for P0 black-box tests"
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    Ok(())
}

include!("p0_feedback_http/part01.rs");
include!("p0_feedback_http/part02.rs");
include!("p0_feedback_http/part03.rs");
