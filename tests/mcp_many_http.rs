//! requirement:read-contract over a real server: one joppa_objects read gives 1 to 25 objects,
//! `fields` names the keys of an object read by its address, and no list reply is above
//! 256 KB, while one object comes in full.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

async fn read(w: &record_world::World, args: Value) -> Result<Result<Value, String>> {
    w.mcp(&w.bot, "joppa_objects", args).await
}

async fn ok(w: &record_world::World, args: Value) -> Result<Value> {
    read(w, args).await?.map_err(anyhow::Error::msg)
}

async fn save(w: &record_world::World, id: &str, body: &str, relations: Value) -> Result<()> {
    let save = json!({"op":"save_object","kind":"requirement","id":id,"revision":0,
        "parent":null,"data":{"title":id,"owner":"owner","body":body},"relations":relations,
        "reason":"Add"});
    w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    Ok(())
}

include!("mcp_many_http/part01.rs");
include!("mcp_many_http/part02.rs");
