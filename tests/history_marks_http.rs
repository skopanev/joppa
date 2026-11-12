//! requirement ea0ca1c0 over a real server: joppa_history_mark marks up to 25 positions in one
//! call, and a refused call marks nothing.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use record_world::World;
use serde_json::{Value, json};

async fn mark(w: &World, args: Value) -> Result<Result<Value, String>> {
    w.mcp(&w.bot, "joppa_history_mark", args).await
}

/// The HTTP History entries of W after `after`, as member alex reads them.
async fn entries(w: &World, after: u64) -> Result<Vec<Value>> {
    let url = format!("{}/api/workspaces/{}/history?after={after}", w.root, w.ws);
    let reply = w.http.get(url).bearer_auth(&w.alex).send().await?;
    let page: Value = reply.error_for_status()?.json().await?;
    Ok(page["items"].as_array().cloned().unwrap_or_default())
}

/// The last journal position of W.
async fn head(w: &World) -> Result<i64> {
    let sql = "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1";
    Ok(w.pool.get().await?.query_one(sql, &[&w.ws]).await?.get(0))
}

/// The required list of joppa_history_mark in tools/list.
async fn required(w: &World) -> Result<Value> {
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    let request = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.bot);
    let reply: Value = request.json(&body).send().await?.json().await?;
    let tools = reply["result"]["tools"].as_array().context("tools")?;
    let tool = tools.iter().find(|t| t["name"] == "joppa_history_mark");
    Ok(tool.context("joppa_history_mark")?["inputSchema"]["required"].clone())
}

// ac:zwtluxs9
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn one_mark_closes_up_to_25_positions() -> Result<()> {
    let w = record_world::world(&[]).await?;
    ensure!(
        required(&w).await? == json!(["workspace", "reason"]),
        "tools/list"
    );
    let revision = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let revision = revision.map_err(anyhow::Error::msg)?["schema"]["revision"].clone();
    let task = json!({"title":"Tasks","role":"task","status":"none","parents":["ac"]});
    let schema = json!({"op":"set_tree_schema","revision":revision,"put_types":{"task":task},
        "reason":"Add Tasks"});
    w.command(&w.owner, schema)
        .await?
        .map_err(anyhow::Error::msg)?;
    let t = json!({"op":"save_object","kind":"task","id":"t","revision":0,"parent":"ac:a",
        "data":{"title":"T","owner":"owner","body":"Do A"},"reason":"Work"});
    w.command(&w.bot, t).await?.map_err(anyhow::Error::msg)?;
    // Entries 1 to 12 of the test: 12 saves. Entry 1 is processed, entries 2 to 12 are open.
    let mut p = Vec::new();
    for i in 1..=12 {
        let save = json!({"op":"save_object","kind":"requirement","id":format!("e{i:02}"),
            "revision":0,"parent":null,"data":{"title":"E","owner":"owner","body":"Rule"},"reason":"Entry"});
        let reply = w.command(&w.bot, save).await?.map_err(anyhow::Error::msg)?;
        p.push(reply["position"].as_u64().context("position")?);
    }
    mark(&w, json!({"position":p[0],"reason":"First"}))
        .await?
        .map_err(anyhow::Error::msg)?;
    let before = head(&w).await?;
    let ten: Vec<u64> = p[1..11].to_vec();
    let args = json!({"positions":ten,"reason":"Read","nodes":["task:t"]});
    let reply = mark(&w, args).await?.map_err(anyhow::Error::msg)?;
    ensure!(
        reply["positions"] == json!(ten)
            && reply["status"] == "processed"
            && reply["at"].is_string()
            && reply["nodes"] == json!(["task:t"])
            && reply.get("reason").is_none(),
        "{reply}"
    );
    let after = p[0] - 1;
    for item in entries(&w, after).await? {
        let position = item["position"].as_u64().unwrap_or(0);
        if !ten.contains(&position) {
            continue;
        }
        let by = &item["processed"];
        ensure!(
            item["status"] == "processed"
                && by["actor"] == "bot"
                && by["reason"] == "Read"
                && by["nodes"] == json!(["task:t"]),
            "{item}"
        );
    }
    let last = p[11];
    let many: Vec<u64> = (1..=26).collect();
    let refusals = [
        (
            json!({"positions":[last, p[0]]}),
            "the entry is already processed",
        ),
        (
            json!({"positions":[last, 999]}),
            "position 999 is not a journal entry",
        ),
        (json!({"positions":[last, last]}), ""),
        (json!({"positions":[]}), "positions holds 1 to 25 entries"),
        (json!({"positions":many}), "positions holds 1 to 25 entries"),
        (
            json!({"position":last,"positions":[last]}),
            "send position or positions",
        ),
    ];
    for (mut args, error) in refusals {
        args["reason"] = json!("Read");
        let refused = mark(&w, args.clone()).await?;
        ensure!(
            refused.as_ref().is_err_and(|e| e.contains(error)),
            "{args}: {refused:?}"
        );
        // A refused call marks nothing: entry 12 stays open.
        let open = entries(&w, after).await?;
        let twelve = open.iter().find(|i| i["position"] == last);
        ensure!(
            twelve.is_some_and(|i| i["status"] == "open"),
            "{args}: entry 12 {twelve:?}"
        );
    }
    ensure!(head(&w).await? == before, "the journal position changed");
    Ok(())
}
