// Part 7 of tests/blocked_task_restart.rs, included there, so the test names stay unchanged.

// claims-persist-replay
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_holder_done_work_comments_and_test_results_survive_a_journal_rebuild() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let f = Fixture {
        pool,
        ws: format!("claims-restart-{}", uuid::Uuid::new_v4().simple()),
    };

    // One ready criterion for each Task. Criterion A binds two tests.
    store::execute_input(
        &f.pool,
        &f.ws,
        OWNER,
        "workspace",
        &json!({"op":"workspace","title":"Claims across a restart"}),
    )
    .await?;
    f.write(
        OWNER,
        json!({"op":"enroll_service","id":HOLDER,"role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(&f.pool, &f.ws, OWNER, None).await?;
    f.write(OWNER, json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    f.write(OWNER, json!({"op":"save_object","kind":"spec","id":"one","revision":0,"parent":null,"data":{"title":"One rule","body":"Implement the agreed rule","owner":OWNER},"reason":"Set the rule"})).await?;
    for id in ["a", "b"] {
        f.write(OWNER, json!({"op":"save_object","kind":"criterion","id":id,"revision":0,"parent":"spec:one","data":{"title":id,"owner":OWNER},"reason":"Set the criterion"})).await?;
    }
    f.write(HOLDER, json!({"op":"set_ac_tests","target":"criterion:a","tests":CHECKS,"reason":"Bind the tests"})).await?;
    let revision = f.read("spec:one").await?["process"]["revision"].clone();
    f.write(
        OWNER,
        json!({"op":"confirm","req":"spec:one","revision":revision}),
    )
    .await?;
    for (task, criterion) in [("t1", "criterion:a"), ("t2", "criterion:b")] {
        f.write(OWNER, json!({"op":"assess","req":"spec:one","revision":revision,"ac":criterion,"assessment":{"ready":true,"reason":"Known scope"}})).await?;
        f.write(OWNER, json!({"op":"save_object","kind":"work","id":task,"revision":0,"parent":criterion,"data":{"title":task,"owner":OWNER,"body":"Implement the agreed mechanism","purpose":"implementation"},"reason":"Set the work contract"})).await?;
    }
    // P completes T2 and then holds T1. T1 has a comment and a reply.
    f.task(HOLDER, "work:t2", json!({"op":"claim_task"}))
        .await?;
    f.task(
        HOLDER,
        "work:t2",
        json!({"op":"complete_task","commit":"c0ffee0","evidence":"Implemented the agreed mechanism"}),
    )
    .await?;
    f.task(HOLDER, "work:t1", json!({"op":"claim_task"}))
        .await?;
    f.write(
        OWNER,
        json!({"op":"note","target":"work:t1","id":"c1","text":"Keep the old sign-in path"}),
    )
    .await?;
    f.write(HOLDER, json!({"op":"note","target":"work:t1","id":"c2","text":"The old path stays","reply_to":"c1"}))
        .await?;

    // The owner reads the workspace through one bearer token on HTTP and MCP.
    let dir = std::env::temp_dir().join(format!(
        "joppa-claims-restart-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir)?;
    let (token, holder) = (
        uuid::Uuid::new_v4().simple().to_string(),
        uuid::Uuid::new_v4().simple().to_string(),
    );
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({ token.clone(): OWNER, holder.clone(): HOLDER }).to_string(),
    )?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let server = start(&url, &tokens, port).await?;
    // P records the results of both tests of A: one passes and one fails.
    let results = json!([{"test":CHECKS[0],"status":"pass"},{"test":CHECKS[1],"status":"fail","message":"refused"}]);
    record_http::recorded(&http, &root, &f.ws, &holder, "c1c1c1c", results).await?;

    let addresses = || async {
        let rows = f
            .pool
            .get()
            .await?
            .query(
                "SELECT address FROM joppa.objects WHERE workspace=$1 ORDER BY address",
                &[&f.ws],
            )
            .await?;
        Ok::<Vec<String>, anyhow::Error>(rows.iter().map(|r| r.get(0)).collect())
    };
    let listed = addresses().await?;
    for address in [
        "spec:one",
        "criterion:a",
        "criterion:b",
        "work:t1",
        "work:t2",
    ] {
        ensure!(
            listed.iter().any(|a| a == address),
            "{address} is missing: {listed:?}"
        );
    }
    let read = || async {
        let mut out = BTreeMap::new();
        for (transport, mcp) in [("HTTP", false), ("MCP", true)] {
            out.insert(
                transport,
                journal_snapshot(&http, &root, &f.ws, &token, mcp, &listed).await?,
            );
        }
        Ok::<_, anyhow::Error>(out)
    };
    let check = |point: &str, out: &BTreeMap<&str, BTreeMap<String, Value>>| {
        for (transport, s) in out {
            let t1 = &s["task work:t1"];
            let comments = &t1["comments"];
            ensure!(
                t1["status"] == "in_progress"
                    && t1["assignee"] == HOLDER
                    && comments.as_object().map(|c| c.len()) == Some(2)
                    && comments["c1"]["text"] == "Keep the old sign-in path"
                    && comments["c1"]["actor"] == OWNER
                    && comments["c2"]["text"] == "The old path stays"
                    && comments["c2"]["actor"] == HOLDER
                    && comments["c2"]["reply_to"] == "c1",
                "{point} {transport} T1: {t1}"
            );
            let t2 = &s["task work:t2"];
            ensure!(
                t2["status"] == "done" && t2["assignee"] == HOLDER,
                "{point} {transport} T2: {t2}"
            );
        }
        for task in ["task work:t1", "task work:t2"] {
            ensure!(
                out["HTTP"][task] == out["MCP"][task],
                "{point} {task}: HTTP {} differs from MCP {}",
                out["HTTP"][task],
                out["MCP"][task]
            );
        }
        let tests = &out["HTTP"]["tests criterion:a"];
        let results: Vec<(&str, &str)> = tests["tests"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| Some((t["address"].as_str()?, t["status"].as_str()?)))
            .collect();
        ensure!(
            results == [(CHECKS[0], "pass"), (CHECKS[1], "fail")] && tests["status"] == "red",
            "{point} GET /tests?ac=criterion:a: {tests}"
        );
        Ok::<_, anyhow::Error>(())
    };

    let before = read().await?;
    check("before the restart", &before)?;
    store::check_journal(&f.pool).await?;
    drop(server);
    f.clear_projections().await?;
    let server = start(&url, &tokens, port).await?;
    ensure!(
        addresses().await? == listed,
        "startup did not rebuild every object"
    );
    let after = read().await?;
    drop(server);
    // Report every restart difference by its read and field before the fixture values are checked.
    let mut changed = vec![];
    for (transport, reads) in &before {
        ensure!(
            after[transport].keys().eq(reads.keys()),
            "{transport}: the restart changed the set of reads"
        );
        for (name, old) in reads {
            differences(
                &format!("{transport} {name}"),
                old,
                &after[transport][name],
                &mut changed,
            );
        }
    }
    ensure!(
        changed.is_empty(),
        "the restart changed {} values:\n{}",
        changed.len(),
        changed.join("\n")
    );
    check("after the restart", &after)?;
    store::check_journal(&f.pool).await?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
