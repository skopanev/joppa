// Part 2 of tests/blocked_task_restart.rs, included there, so the test names stay unchanged.

async fn snapshot(
    http: &reqwest::Client,
    root: &str,
    ws: &str,
    token: &str,
    mcp: bool,
) -> Result<Snapshot> {
    let get = |path: String, tool: &'static str, arguments: Value| {
        fetch(http, root, ws, token, mcp, (path, tool, arguments))
    };
    let all = get(
        "tasks?view=all&limit=100".into(),
        "joppa_tasks",
        json!({"view":"all","limit":100}),
    )
    .await?;
    let available = get(
        "tasks?view=available&limit=100".into(),
        "joppa_tasks",
        json!({"view":"available","limit":100}),
    )
    .await?;
    let find = |page: &Value, address: &str| {
        listed(page, "tasks")
            .as_array()
            .and_then(|tasks| tasks.iter().find(|t| t["address"] == address))
            .cloned()
    };
    let mut out = Snapshot::new();
    for (name, address, _) in FIXTURES {
        let object = get(
            format!("objects?address={address}"),
            "joppa_objects",
            json!({"address":address}),
        )
        .await?;
        let queued = find(&all, address)
            .ok_or_else(|| anyhow::anyhow!("{name} {address} is missing in view=all: {all}"))?;
        out.insert(
            name,
            json!({
                "status": object["process"]["status"],
                "blocked": object["process"]["blocked"],
                "assignee": object["process"]["assignee"],
                "claimable": queued["claimable"],
                "waiting_reason": queued["waiting_reason"],
                "available": find(&available, address).is_some(),
            }),
        );
    }
    Ok(out)
}

/// The fixture values that each transport must report at each point.
fn expect(
    point: &str,
    transport: &str,
    s: &Snapshot,
    blocks: &BTreeMap<&str, Value>,
) -> Result<()> {
    let at = |name: &str| format!("{point} {transport} {name}: {}", s[name]);
    let f1 = &s["F1"];
    ensure!(
        f1["blocked"] == blocks["F1"]
            && f1["blocked"]["actor"] == L
            && f1["assignee"].is_null()
            && f1["claimable"] == false
            && f1["waiting_reason"] == BLOCKED
            && f1["available"] == false,
        "{}",
        at("F1")
    );
    let f2 = &s["F2"];
    ensure!(
        f2["blocked"].is_null() && f2["waiting_reason"] != BLOCKED,
        "{}",
        at("F2")
    );
    let f3 = &s["F3"];
    ensure!(
        f3["blocked"] == blocks["F3"]
            && f3["blocked"]["actor"] == OWNER
            && f3["assignee"].is_null()
            && f3["claimable"] == false
            && f3["available"] == false,
        "{}",
        at("F3")
    );
    let f4 = &s["F4"];
    ensure!(
        f4["status"] == "retired"
            && f4["blocked"] == blocks["F4"]
            && f4["claimable"] == false
            && f4["available"] == false,
        "{}",
        at("F4")
    );
    Ok(())
}
