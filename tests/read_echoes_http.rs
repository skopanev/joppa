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
    let saved = write(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"t","revision":0,"parent":"ac:a",
        "data":{"title":"Do A","owner":"owner","contract":"Make A green","purpose":"implementation","projects":[]},
        "reason":"Create"})).await?;
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
    write(
        &w,
        &w.owner,
        json!({"op":"retire_task","id":"task:t","revision":1,"reason":"Not needed"}),
    )
    .await?;
    // A Task item: the address is the ID, the parent is the criterion, and a retired Task
    // has no waiting reason.
    let tasks = read(&w, "joppa_tasks", json!({"view":"all"})).await?;
    let task = &tasks["items"][0];
    ensure!(
        tasks.get("view").is_none()
            && task["address"] == "task:t"
            && task["parent"] == "ac:a"
            && task["status"] == "retired"
            && absent(task, &["id", "ac", "waiting_reason"])
            && (task.get("readiness_blocker").is_none()
                || task["readiness_blocker"] != task["assessment"])
            && absent(&task["signal"], &["assessment", "readiness_blocker"])
            && task["signal"]["tone"].is_string(),
        "{tasks}"
    );
    // A record names itself only by its address.
    w.recorded(
        &"a".repeat(40),
        json!([{"test":"tests/x.rs > t","status":"pass"}]),
    )
    .await?;
    let timeline = read(
        &w,
        "joppa_timeline",
        json!({"order":"newest","limit":1,"after":0}),
    )
    .await?;
    let entry = &timeline["items"][0];
    ensure!(
        absent(&timeline, &["after", "order"])
            && entry["address"]
                .as_str()
                .is_some_and(|a| a.starts_with("record/"))
            && absent(entry, &["record", "report"]),
        "{timeline}"
    );
    // History, the tests overview and links give no workspace and no snapshot position.
    for args in [json!({}), json!({"all":true,"limit":5})] {
        let history = read(&w, "joppa_history", args).await?;
        ensure!(
            absent(&history, &["workspace", "position", "all", "limit"])
                && history["items"].as_array().is_some_and(|items| {
                    !items.is_empty() && items.iter().all(|i| absent(i, &["signal"]))
                }),
            "{history}"
        );
    }
    let overview = read(&w, "joppa_tests", json!({})).await?;
    ensure!(absent(&overview, &["workspace", "position"]), "{overview}");
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
    // The test results decide a criterion: its process gives no run status or revision,
    // and assessment gives the requirement revision that assess sends.
    let ac = w.object("ac:a").await?;
    ensure!(
        absent(
            &ac["process"],
            &["revision", "status", "verification", "pending_run"]
        ) && ac["process"]["assessment"]["revision"].is_u64(),
        "{ac}"
    );
    // A requirement gives no copy of its current revision, and false review_required goes.
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

// read-mcp-echoes
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_requirement_list_with_data_omits_the_old_consumers_field() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    write(
        &w,
        &w.bot,
        json!({"op":"save_object","kind":"requirement","id":"old","revision":0,"parent":null,
        "data":{"title":"Old","owner":"owner","body":"Old rule","consumers":[]},"reason":"Create"}),
    )
    .await?;
    let list = read(
        &w,
        "joppa_objects",
        json!({"kind":"requirement","parent":"*","fields":["data"]}),
    )
    .await?;
    let items = list["items"].as_array().cloned().unwrap_or_default();
    let old = items.iter().find(|i| i["address"] == "requirement:old");
    ensure!(
        old.is_some_and(|i| i["data"]["body"] == "Old rule" && i["data"].get("consumers").is_none()),
        "{list}"
    );
    let detail = w.object("requirement:old").await?;
    ensure!(detail["data"].get("consumers").is_none(), "{detail}");
    Ok(())
}
