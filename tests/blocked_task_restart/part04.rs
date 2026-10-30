// Part 4 of tests/blocked_task_restart.rs, included there, so the test names stay unchanged.

/// The planning claim, unstarted work, Task state and queue values of each plan.
async fn planning_snapshot(
    http: &reqwest::Client,
    root: &str,
    ws: &str,
    token: &str,
    mcp: bool,
) -> Result<Snapshot> {
    let get = |path: String, tool: &'static str, arguments: Value| {
        fetch(http, root, ws, token, mcp, (path, tool, arguments))
    };
    let mut pages = BTreeMap::new();
    for view in ["all", "available"] {
        let page = get(
            format!("tasks?view={view}&limit=100"),
            "joppa_tasks",
            json!({"view":view,"limit":100}),
        )
        .await?;
        pages.insert(view, listed(&page, "tasks").clone());
    }
    let unstarted = get(
        "objects?work_filter=unstarted&limit=100".into(),
        "joppa_objects",
        json!({"work_filter":"unstarted","limit":100}),
    )
    .await?;
    pages.insert("unstarted", unstarted["items"].clone());
    let find = |page: &str, address: &str| {
        pages[page]
            .as_array()
            .and_then(|items| items.iter().find(|i| i["address"] == address))
            .cloned()
    };
    let mut out = Snapshot::new();
    for (name, criterion, task) in PLANS {
        let object = |address: &str| {
            get(
                format!("objects?address={address}"),
                "joppa_objects",
                json!({"address":address}),
            )
        };
        let c = object(criterion).await?;
        let mut fields = json!({
            "planning_claim": c["process"]["planning_claim"],
            "unstarted": find("unstarted", criterion).is_some(),
        });
        if let Some(task) = task {
            let t = object(task).await?;
            let queued = find("all", task)
                .ok_or_else(|| anyhow::anyhow!("{name} {task} is missing in view=all"))?;
            fields["status"] = t["process"]["status"].clone();
            fields["assignee"] = t["process"]["assignee"].clone();
            // MCP omits claimable when it is false.
            fields["claimable"] = json!(queued["claimable"] == true);
            fields["waiting_reason"] = queued["waiting_reason"].clone();
            fields["available"] = json!(find("available", task).is_some());
        }
        out.insert(name, fields);
    }
    Ok(out)
}

/// The plan values that each transport must report at each point.
fn expect_plans(
    point: &str,
    transport: &str,
    s: &Snapshot,
    claims: &BTreeMap<&str, Value>,
) -> Result<()> {
    let expected = BTreeMap::from([
        (
            "held",
            json!({"planning_claim":claims["held"],"unstarted":false,"status":"open","assignee":null,
                "claimable":false,"waiting_reason":AWAITING,"available":false}),
        ),
        (
            "finished",
            json!({"planning_claim":null,"unstarted":false,"status":"open","assignee":null,
                "claimable":true,"waiting_reason":null,"available":true}),
        ),
        (
            "released",
            json!({"planning_claim":null,"unstarted":true,"status":"retired","assignee":null,
                "claimable":false,"waiting_reason":null,"available":false}),
        ),
        (
            "claimed",
            json!({"planning_claim":claims["claimed"],"unstarted":false}),
        ),
    ]);
    for (name, fields) in &expected {
        ensure!(
            &s[name] == fields,
            "{point} {transport} {name}: expected {fields}, got {}",
            s[name]
        );
    }
    Ok(())
}
