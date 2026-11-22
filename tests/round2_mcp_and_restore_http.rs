//! Black-box HTTP and MCP tests for the four MCP protocol fixes (ac:dsj99sz6, ac:yyfmvx37,
//! ac:m3c0yv7q, ac:yg6tawf1) and restore_object across all roles (ac:i2gz323e).
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/save_world.rs"]
mod save_world;
use anyhow::{Context, Result, ensure};
use record_world::World;
use save_world::{batch, head, ok, refused, replay};
use serde_json::{Value, json};

async fn mcp_ok(w: &World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args)
        .await?
        .map_err(|e| anyhow::anyhow!("{tool}: {e}"))
}

async fn tools_list(w: &World) -> Result<Vec<Value>> {
    let raw: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    raw["result"]["tools"]
        .as_array()
        .cloned()
        .context("missing tools array")
}

include!("round2_mcp_and_restore_http/part01.rs");
include!("round2_mcp_and_restore_http/part02.rs");
include!("round2_mcp_and_restore_http/part03.rs");
