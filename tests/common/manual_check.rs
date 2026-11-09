//! Manual checks over a real server: joppa_attach with check_result and valid_until, and the
//! status of a criterion from store::test_status and the joppa_objects tone.
#![allow(dead_code)]
use crate::record_world::World;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

/// The UTC date `days` after today, as YYYY-MM-DD.
pub fn day(days: i64) -> String {
    (chrono::Utc::now().date_naive() + chrono::TimeDelta::days(days))
        .format("%Y-%m-%d")
        .to_string()
}

/// joppa_attach of a new text file to `target` with the extra fields of `fields`, such as
/// check_result and valid_until. A refusal gives Err with the exact error text.
pub async fn attach(
    w: &World,
    token: &str,
    target: &str,
    fields: Value,
) -> Result<Result<Value, String>> {
    let content = format!("Manual check {}", uuid::Uuid::new_v4());
    let mut args = json!({"target":target,"filename":"check.txt","content_type":"text/plain",
        "content":content,"request_id":uuid::Uuid::new_v4().to_string()});
    for (k, v) in fields.as_object().into_iter().flatten() {
        args[k] = v.clone();
    }
    w.mcp(token, "joppa_attach", args).await
}

/// A joppa_attach of bot that must succeed. Gives the reply.
pub async fn check(w: &World, target: &str, fields: Value) -> Result<Value> {
    let text = fields.to_string();
    attach(w, &w.bot, target, fields)
        .await?
        .map_err(|e| anyhow::anyhow!("attach {text} to {target}: {e}"))
}

/// The status and the reason of `address` from store::test_status, and its joppa_objects tone.
pub async fn state(w: &World, address: &str) -> Result<(Value, Value, Value)> {
    let status = joppa::store::test_status(&w.pool, &w.ws, "owner", address).await?;
    let tone = w.object(address).await?["status"]["tone"].clone();
    Ok((status["status"].clone(), status["reason"].clone(), tone))
}

/// `address` has the status `want` in store::test_status and as its joppa_objects tone. With
/// `reason`, test_status gives that reason.
pub async fn expect(
    w: &World,
    step: &str,
    address: &str,
    want: &str,
    reason: Option<&str>,
) -> Result<()> {
    let (status, why, tone) = state(w, address).await?;
    ensure!(
        status == want && tone == want && reason.is_none_or(|r| why == r),
        "{step}: {address} must be {want} {reason:?}: status {status}, reason {why}, tone {tone}"
    );
    Ok(())
}

/// The joppa_objects tone of `address`.
pub async fn tone(w: &World, address: &str) -> Result<Value> {
    Ok(w.object(address).await?["status"]["tone"].clone())
}

/// A new criterion under requirement:r with one test node for each of `tests`.
pub async fn criterion(w: &World, id: &str, tests: &[&str]) -> Result<()> {
    let save = json!({"op":"save_object","kind":"ac","id":id,"revision":0,"parent":"requirement:r",
        "data":{"title":id.to_uppercase(),"owner":"owner"},"reason":"Create the criterion"});
    w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
    for test in tests {
        let bind = json!({"op":"save_object","kind":"test","revision":0,"parent":format!("ac:{id}"),
            "data":{"title":test,"owner":"owner"},"reason":"Bind the test"});
        w.command(&w.bot, bind).await?.map_err(anyhow::Error::msg)?;
    }
    Ok(())
}

/// The owner saves `address` with a new body. The other fields stay.
pub async fn new_body(w: &World, address: &str, body: &str) -> Result<Value> {
    let current = w.object(address).await?;
    let (kind, id) = address.split_once(':').context("address")?;
    let save = json!({"op":"save_object","kind":kind,"id":id,"revision":current["revision"],
        "data":{"body":body},"reason":"Change the text"});
    w.command(&w.owner, save)
        .await?
        .map_err(|e| anyhow::anyhow!("save {address}: {e}"))
}

/// The files of `target` from joppa_attachments for bot.
pub async fn files(w: &World, target: &str) -> Result<Vec<Value>> {
    let reply = w
        .mcp(&w.bot, "joppa_attachments", json!({"target":target}))
        .await?
        .map_err(anyhow::Error::msg)?;
    reply["files"].as_array().cloned().context("files")
}
