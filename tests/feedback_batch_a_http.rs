//! Black-box integration checks for Batch A (source:bb49yruy):
//! list detail fields (ac:d6n5bm15), redaction across comments, intake and search (ac:zl90a6on),
//! slash aliases (ac:m6goiyaq), generalized forced_revision in batches (ac:bd6w8s1v),
//! 100-address reads with missing (ac:a3qp8jxw), History limit 100 (ac:mhuj6ldr),
//! and whole-tree group_by counts (ac:7b8q9bq5).
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};

async fn ok(w: &record_world::World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args).await?.map_err(anyhow::Error::msg)
}

include!("feedback_batch_a_http/part01.rs");
include!("feedback_batch_a_http/part02.rs");
