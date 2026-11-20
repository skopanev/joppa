//! Black-box integration checks for Batches B and C (source:bhue8kvd):
//! commit_required false (ac:ua662yvd), contract_only_reset and type reset_on_change (ac:szu8fkbc),
//! list filters not_data, has_tests and comment_prefix (ac:l6drwbo5),
//! History filters by_alias, op, kind and target (ac:pquuuvss),
//! archived search ranking and present false (ac:y3jrsfze),
//! comments_active and comments_latest (ac:qiq0u7w9),
//! and separated authored data vs computed on MCP reads (ac:478njhfy).
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

async fn ok(w: &record_world::World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args).await?.map_err(anyhow::Error::msg)
}

include!("feedback_batch_bc_http/part01.rs");
include!("feedback_batch_bc_http/part02.rs");
