//! Black-box HTTP and MCP tests for replay epoch own_requirement_revisions (ac:drr28sx9)
//! and History subtree filter under (ac:rghuniww).
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/save_world.rs"]
mod save_world;
use anyhow::{Context, Result, ensure};
use record_world::World;
use save_world::{append_event, head, item_id, ok, refused, replay, revisions};
use serde_json::{Value, json};

async fn mcp_ok(w: &World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args)
        .await?
        .map_err(|e| anyhow::anyhow!("{tool}: {e}"))
}

include!("own_requirement_revisions_http/part01.rs");
include!("own_requirement_revisions_http/part02.rs");
