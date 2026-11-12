//! tac:661jb0ex over a real server: an MCP write builds no summary and no system_ids, and it
//! keeps the item ID registration. POST /api/workspaces/{w}/commands keeps both keys.
//! pg_stat_statements counts the statements, as tests/read_latency.rs reads it.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/statement_count.rs"]
mod statement_count;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use statement_count::{ONE, install, measure};

/// The statement that builds system_ids from the item IDs.
const SYSTEM_IDS: &str = "select local_id,id from joppa.item_ids";

/// A joppa_batch of 60 new requirements with the ids `<round>00` to `<round>59`.
fn batch(round: &str) -> Value {
    let operations: Vec<Value> = (0..60)
        .map(|i| json!({"command":{"op":"save_object","kind":"requirement","id":format!("{round}{i:02}"),
            "revision":0,"parent":null,"data":{"title":format!("Receipt {round}{i:02}"),"owner":"owner",
            "body":"A rule"},"reason":"Create"}}))
        .collect();
    json!({"request_id":format!("receipt-{round}"),"operations":operations})
}

/// The reply has 60 results, and neither the reply nor a result has summary or system_ids.
fn dry(reply: &Value) -> bool {
    let bare = |v: &Value| v.get("summary").is_none() && v.get("system_ids").is_none();
    bare(reply)
        && reply["results"]
            .as_array()
            .is_some_and(|r| r.len() == 60 && r.iter().all(bare))
}

/// The reply without the replayed flag of a retry.
fn settled(mut reply: Value) -> Value {
    if let Some(object) = reply.as_object_mut() {
        object.remove("replayed");
    }
    let results = reply.get_mut("results").and_then(Value::as_array_mut);
    for result in results.into_iter().flatten() {
        if let Some(object) = result.as_object_mut() {
            object.remove("replayed");
        }
    }
    reply
}

async fn mcp(w: &record_world::World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args).await?.map_err(anyhow::Error::msg)
}

// tac:661jb0ex
#[tokio::test]
#[ignore = "requires isolated PostgreSQL with pg_stat_statements preloaded"]
async fn an_mcp_write_builds_no_summary_and_no_system_ids_and_registers_each_item_id() -> Result<()>
{
    let _one = ONE.lock().await;
    let w = record_world::world(&[]).await?;
    install(&w.pool).await?;
    // Each measured call saves 60 new objects: register_item runs 60 times, no item_ids select.
    let round = std::cell::Cell::new(0u32);
    let tool = async || {
        let n = round.get();
        round.set(n + 1);
        let reply = mcp(&w, "joppa_batch", batch(&format!("m{n}x"))).await?;
        ensure!(dry(&reply), "{reply}");
        Ok(())
    };
    let found = measure(&w.pool, tool).await?;
    ensure!(
        found.naming(&["joppa.register_item"]) == 60 && found.naming(&[SYSTEM_IDS]) == 0,
        "MCP: {found}"
    );
    // A retry with the same request_id gives an equal reply from the stored receipt.
    let first = mcp(&w, "joppa_batch", batch("new")).await?;
    let again = mcp(&w, "joppa_batch", batch("new")).await?;
    ensure!(
        dry(&first) && dry(&again) && settled(first.clone()) == settled(again.clone()),
        "{first} {again}"
    );
    // Each stored receipt of a batch holds the reply without summary and system_ids.
    let conn = w.pool.get().await?;
    let sql = "SELECT result::text FROM joppa.command_receipts WHERE workspace=$1 AND input->>'batch'='true'";
    let receipts = conn.query(sql, &[&w.ws]).await?;
    ensure!(receipts.len() == 5, "{} batch receipts", receipts.len());
    for row in &receipts {
        let stored: Value = serde_json::from_str(row.get(0))?;
        ensure!(dry(&stored), "receipt: {stored}");
    }
    // Each new object reads by its public item ID.
    let sql = "SELECT local_id,id FROM joppa.item_ids WHERE workspace=$1 AND kind='requirement' AND local_id LIKE 'new%'";
    let ids = conn.query(sql, &[&w.ws]).await?;
    ensure!(ids.len() == 60, "{} item IDs", ids.len());
    for row in &ids {
        let (local, public): (String, String) = (row.get(0), row.get(1));
        let read = mcp(&w, "joppa_objects", json!({"address":public})).await?;
        ensure!(
            read["data"]["title"] == format!("Receipt {local}"),
            "{public}: {read}"
        );
    }
    // An HTTP save keeps summary and system_ids.
    let save = json!({"op":"save_object","kind":"requirement","id":"web","revision":0,"parent":null,
        "data":{"title":"Web","owner":"owner","body":"A rule"},"reason":"Create"});
    let url = format!("{}/api/workspaces/{}/commands", w.root, w.ws);
    let body = json!({"request_id":"receipt-web","command":save});
    let reply = w
        .http
        .post(url)
        .bearer_auth(&w.bot)
        .json(&body)
        .send()
        .await?;
    let code = reply.status();
    let saved: Value = reply.json().await?;
    ensure!(
        code.is_success()
            && saved.get("summary").is_some()
            && saved["system_ids"]["requirement"]["web"].is_string(),
        "HTTP: {code} {saved}"
    );
    Ok(())
}
