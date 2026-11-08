//! requirement:read-contract over a real server: MCP replies repeat no fact and echo no
//! request argument, no read gives its workspace or a snapshot position, and a write is small.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

async fn read(w: &record_world::World, tool: &str, args: Value) -> Result<Value> {
    w.mcp(&w.bot, tool, args).await?.map_err(anyhow::Error::msg)
}

async fn write(w: &record_world::World, token: &str, command: Value) -> Result<Value> {
    w.command(token, command).await?.map_err(anyhow::Error::msg)
}

fn absent(value: &Value, keys: &[&str]) -> bool {
    keys.iter().all(|k| value.get(*k).is_none())
}

// read-mcp-echoes
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn mcp_replies_repeat_no_fact_and_echo_no_argument() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    // A schema write gives the new schema revision and not the sent types.
    let tree = read(&w, "joppa_tree", json!({})).await?;
    let revision = tree["schema"]["revision"].as_u64().unwrap_or_default();
    let schema = write(
        &w,
        &w.owner,
        json!({"op":"set_tree_schema","revision":revision,"reason":"Add Tasks",
        "put_types":{"task":{"title":"Tasks","role":"task","status":"none","parents":["ac"]}}}),
    )
    .await?;
    ensure!(
        schema["schema_revision"] == revision + 1
            && schema["command"]
                == json!({"op":"set_tree_schema","revision":revision,"reason":"Add Tasks"})
            && absent(&schema, &["summary", "system_ids"]),
        "{schema}"
    );
    let tree = read(&w, "joppa_tree", json!({})).await?;
    ensure!(tree["schema"]["revision"] == revision + 1, "{tree}");
    // An object write gives no summaries and no public IDs, also inside a batch.
    let saved = write(
        &w,
        &w.bot,
        json!({"op":"save_object","kind":"task","id":"t","revision":0,"parent":"ac:a",
        "data":{"title":"Do A","owner":"owner","body":"Make A green","projects":[]},
        "reason":"Create"}),
    )
    .await?;
    ensure!(
        saved["object"]["address"] == "task:t" && absent(&saved, &["summary", "system_ids"]),
        "{saved}"
    );
    for half in 0..2 {
        let operations: Vec<Value> = (0..60)
            .map(|i| json!({"command":{"op":"save_object","kind":"ac","id":format!("p{half}{i:02}"),"revision":0,
                "parent":"requirement:r","data":{"title":format!("Paging item {half}{i:02}"),"owner":"owner"},
                "reason":"Paging test"}}))
            .collect();
        let batch = w
            .mcp(
                &w.bot,
                "joppa_batch",
                json!({"request_id":format!("echoes-{half}"),"operations":operations}),
            )
            .await?
            .map_err(anyhow::Error::msg)?;
        let results = batch["results"].as_array().cloned().unwrap_or_default();
        ensure!(
            results.len() == 60
                && absent(&batch, &["summary", "system_ids"])
                && results.iter().all(|r| absent(r, &["system_ids"])),
            "{batch}"
        );
    }
    // A record names itself only by its address.
    w.recorded(
        &"a".repeat(40),
        json!([{"test":"tests/x.rs > t","status":"pass"}]),
    )
    .await?;
    let newest = read(
        &w,
        "joppa_history",
        json!({"order":"newest","limit":1,"after":0}),
    )
    .await?;
    let entry = &newest["items"][0];
    ensure!(
        absent(&newest, &["after", "order", "open", "limit"])
            && entry["address"]
                .as_str()
                .is_some_and(|a| a.starts_with("record/"))
            && absent(entry, &["record", "report"]),
        "{newest}"
    );
    // History and links give no workspace and no snapshot position.
    for args in [json!({}), json!({"open":true,"limit":5})] {
        let history = read(&w, "joppa_history", args).await?;
        ensure!(
            absent(
                &history,
                &["workspace", "position", "open", "order", "after", "limit"]
            ) && history["items"].as_array().is_some_and(|items| {
                !items.is_empty() && items.iter().all(|i| absent(i, &["signal"]))
            }),
            "{history}"
        );
    }
    let links = read(&w, "joppa_objects", json!({"links_of":"ac:a"})).await?;
    ensure!(
        absent(&links, &["workspace", "position", "address"]),
        "{links}"
    );
    // Search counts every text match, and echoes no flag.
    let found = read(&w, "joppa_search", json!({"query":"Paging item"})).await?;
    ensure!(
        found["total"].as_u64() >= Some(120)
            && absent(&found, &["all_versions", "all_workspaces", "workspaces"]),
        "{found}"
    );
    // The test results decide a criterion: its process gives no run status or revision.
    let ac = w.object("ac:a").await?;
    ensure!(
        absent(
            &ac["process"],
            &[
                "revision",
                "status",
                "verification",
                "pending_run",
                "assessment"
            ]
        ),
        "{ac}"
    );
    // A requirement gives no copy of its current revision and no review_required.
    let requirement = w.object("requirement:r").await?;
    ensure!(
        absent(
            &requirement["process"],
            &["summary", "verification", "revision"]
        ) && requirement["revision"].is_u64()
            && absent(&requirement, &["review_required"])
            && absent(&ac, &["review_required"]),
        "{requirement}"
    );
    Ok(())
}
