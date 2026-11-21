//! Workspace write key (`jpt_<64hex>`) and `key_write` schema rules over a real HTTP/MCP server.
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use read_key::{Auth, call, purge, session};
use record_world::World;
use serde_json::{Value, json};

fn write_key_path(ws: &str, tail: &str) -> String {
    format!("/api/workspaces/{ws}/write-key{tail}")
}

async fn mcp_ok(w: &World, token: &str, tool: &str, args: Value) -> Result<Value> {
    w.mcp(token, tool, args).await?.map_err(anyhow::Error::msg)
}

async fn create_write_key(w: &World, cookie: &str, ws: &str) -> Result<String> {
    let auth = [Auth::Cookie(cookie), Auth::Origin(&w.root)];
    let path = write_key_path(ws, "");
    let (code, body) = call(&w.http, &w.root, "POST", &path, &auth, None).await?;
    ensure!(code == 200, "POST write-key: {code} {body}");
    let key = body["key"].as_str().context("key")?.to_owned();
    ensure!(joppa::write_key::is_write_key(&key), "write key {key:?}");
    Ok(key)
}

async fn revoke_write_key(w: &World, cookie: &str, ws: &str) -> Result<()> {
    let auth = [Auth::Cookie(cookie), Auth::Origin(&w.root)];
    let path = write_key_path(ws, "/revoke");
    let (code, body) = call(&w.http, &w.root, "POST", &path, &auth, None).await?;
    ensure!(code == 200, "POST write-key/revoke: {code} {body}");
    Ok(())
}

async fn write_key_row(w: &World, ws: &str) -> Result<(i64, Option<String>)> {
    let conn = w.pool.get().await?;
    let sql =
        "SELECT count(*), max(token_sha256) FROM joppa.workspace_write_keys WHERE workspace=$1";
    let row = conn.query_one(sql, &[&ws]).await?;
    Ok((row.get(0), row.get(1)))
}

#[path = "write_key_http/part01.rs"]
mod part01;
#[path = "write_key_http/part02.rs"]
mod part02;
