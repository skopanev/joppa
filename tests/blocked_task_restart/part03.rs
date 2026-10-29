// Part 3 of tests/blocked_task_restart.rs, included there, so the test names stay unchanged.

// failed-task-blocks-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn blocked_tasks_keep_their_state_across_a_restart_that_rebuilds_the_journal() -> Result<()> {
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
        ws: format!("blocked-restart-{}", uuid::Uuid::new_v4().simple()),
    };

    // A workspace with custom criterion and Task type names and one ready criterion per fixture.
    store::execute_input(
        &f.pool,
        &f.ws,
        OWNER,
        "workspace",
        &json!({"op":"workspace","title":"Blocked Tasks across a restart"}),
    )
    .await?;
    for actor in [PLANNER, L, Z] {
        f.write(
            OWNER,
            json!({"op":"enroll_service","id":actor,"role":"agent"}),
        )
        .await?;
    }
    let schema = store::tree_schema(&f.pool, &f.ws, OWNER, None).await?;
    f.write(OWNER, json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    f.write(PLANNER, json!({"op":"save_object","kind":"spec","id":"one","revision":0,"parent":null,"data":{"title":"One rule","body":"Implement the agreed rule","owner":OWNER},"reason":"Set the rule"})).await?;
    for (_, _, criterion) in FIXTURES {
        let id = criterion.split_once(':').unwrap().1;
        f.write(PLANNER, json!({"op":"save_object","kind":"criterion","id":id,"revision":0,"parent":"spec:one","data":{"title":id,"owner":OWNER},"reason":"Set the criterion"})).await?;
    }
    let revision = f.read("spec:one").await?["process"]["revision"].clone();
    f.write(
        OWNER,
        json!({"op":"confirm","req":"spec:one","revision":revision}),
    )
    .await?;
    for (_, task, criterion) in FIXTURES {
        f.write(PLANNER, json!({"op":"assess","req":"spec:one","revision":revision,"ac":criterion,"assessment":{"ready":true,"reason":"Known scope"}})).await?;
        let id = task.split_once(':').unwrap().1;
        f.write(PLANNER, json!({"op":"save_object","kind":"work","id":id,"revision":0,"parent":criterion,"data":{"title":id,"owner":OWNER,"contract":"Implement the agreed mechanism","purpose":"implementation"},"reason":"Set the work contract"})).await?;
    }

    let mut blocks = BTreeMap::new();
    // F1: L claims and blocks T1.
    f.task(L, "work:t1", json!({"op":"claim_task"})).await?;
    blocks.insert("F1", f.block(L, "work:t1").await?);
    // F2: L blocks T2; P plans a new T2 contract and finishes planning.
    f.task(L, "work:t2", json!({"op":"claim_task"})).await?;
    f.block(L, "work:t2").await?;
    let ac = f.read("criterion:f2").await?;
    f.write(
        PLANNER,
        json!({"op":"claim_planning","ac":"criterion:f2","revision":ac["current_revision"]}),
    )
    .await?;
    {
        let current = f.read("work:t2").await?;
        let mut data = current["document"]["data"].clone();
        data["title"] = json!("t2");
        data["contract"] = json!("Implement the agreed mechanism with a new fix");
        f.write(PLANNER, json!({"op":"save_object","kind":"work","id":"t2","revision":current["current_revision"],"parent":current["document"]["parent"],"data":data,"links":current["document"]["links"],"reason":"Amend the contract"})).await?;
    }
    f.write(
        PLANNER,
        json!({"op":"finish_planning","ac":"criterion:f2","reason":"Plan complete"}),
    )
    .await?;
    let t2 = f.read("work:t2").await?;
    ensure!(t2["process"]["blocked"].is_null(), "F2: {t2}");
    blocks.insert("F2", Value::Null);
    // F3: Z claims D and the workspace owner S blocks it.
    f.task(Z, "work:d", json!({"op":"claim_task"})).await?;
    blocks.insert("F3", f.block(OWNER, "work:d").await?);
    let d = f.read("work:d").await?;
    ensure!(
        d["process"]["blocked"]["actor"] == OWNER && d["process"]["assignee"].is_null(),
        "F3: {d}"
    );
    // F4: L blocks T4 and S retires it.
    f.task(L, "work:t4", json!({"op":"claim_task"})).await?;
    let block = f.block(L, "work:t4").await?;
    f.task(
        OWNER,
        "work:t4",
        json!({"op":"retire_task","reason":"The planned work replaces this Task"}),
    )
    .await?;
    let t4 = f.read("work:t4").await?;
    ensure!(
        t4["process"]["status"] == "retired" && t4["process"]["blocked"] == block,
        "F4: {t4}"
    );
    blocks.insert("F4", block);

    // Worker Z reads the workspace through one bearer token on HTTP and MCP.
    let dir = std::env::temp_dir().join(format!(
        "joppa-blocked-restart-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir)?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, json!({ token.clone(): Z }).to_string())?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let read = || async {
        let mut out = BTreeMap::new();
        for (transport, mcp) in [("HTTP", false), ("MCP", true)] {
            out.insert(transport, snapshot(&http, &root, &f.ws, &token, mcp).await?);
        }
        Ok::<_, anyhow::Error>(out)
    };
    let check = |point: &str, out: &BTreeMap<&str, Snapshot>| {
        for (transport, s) in out {
            expect(point, transport, s, &blocks)?;
        }
        ensure!(
            out["HTTP"] == out["MCP"],
            "{point}: HTTP {:#?} differs from MCP {:#?}",
            out["HTTP"],
            out["MCP"]
        );
        Ok::<_, anyhow::Error>(())
    };

    let server = start(&url, &tokens, port).await?;
    let before = read().await?;
    check("before the restart", &before)?;
    store::check_journal(&f.pool).await?;
    drop(server);

    f.clear_projections().await?;

    let server = start(&url, &tokens, port).await?;
    let rebuilt = f
        .count("SELECT count(*) FROM joppa.board_cache b WHERE b.workspace=$1 AND b.position=(SELECT MAX(e.position) FROM joppa.events e WHERE e.workspace=b.workspace)")
        .await?;
    ensure!(
        rebuilt == 1,
        "startup did not rebuild board_cache at the last event position"
    );
    let rows = f
        .pool
        .get()
        .await?
        .query(
            "SELECT DISTINCT address FROM joppa.task_queue WHERE workspace=$1",
            &[&f.ws],
        )
        .await?;
    let addresses: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
    for (name, address, _) in FIXTURES {
        ensure!(
            addresses.iter().any(|a| a == address),
            "startup did not rebuild the task_queue row of {name} {address}: {addresses:?}"
        );
    }
    let after = read().await?;
    drop(server);
    // Report a restart difference before the fixture values are checked again.
    for (transport, snapshot) in &before {
        for (name, fields) in snapshot {
            for (field, old) in fields.as_object().unwrap() {
                let new = &after[transport][name][field];
                ensure!(
                    old == new,
                    "{name} {transport} {field}: before the restart {old}, after {new}"
                );
            }
        }
    }
    check("after the restart", &after)?;
    store::check_journal(&f.pool).await?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

const READER: &str = "reader";

const AWAITING: &str = "Task is awaiting planning publication";

/// Planning fixtures with custom type names: each criterion and its new Task.
const PLANS: [(&str, &str, Option<&str>); 4] = [
    ("held", "rule:held", Some("job:held")),
    ("finished", "rule:finished", Some("job:finished")),
    ("released", "rule:released", Some("job:released")),
    ("claimed", "rule:claimed", None),
];
