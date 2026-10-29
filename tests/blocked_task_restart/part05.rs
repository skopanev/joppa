// Part 5 of tests/blocked_task_restart.rs, included there, so the test names stay unchanged.

// planning-claim-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn held_finished_and_released_plans_keep_their_state_across_a_journal_rebuild() -> Result<()>
{
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
        ws: format!("planning-restart-{}", uuid::Uuid::new_v4().simple()),
    };

    // Custom type names with criterion and Task roles; every rule is ready.
    store::execute_input(
        &f.pool,
        &f.ws,
        OWNER,
        "workspace",
        &json!({"op":"workspace","title":"Plans across a restart"}),
    )
    .await?;
    for actor in [PLANNER, READER] {
        f.write(
            OWNER,
            json!({"op":"enroll_service","id":actor,"role":"agent"}),
        )
        .await?;
    }
    let schema = store::tree_schema(&f.pool, &f.ws, OWNER, None).await?;
    f.write(OWNER, json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "story":{"title":"Story","role":"requirement","status":"children","root":true,"parents":[]},
        "rule":{"title":"Rule","role":"ac","status":"tests","parents":["story"]},
        "job":{"title":"Job","role":"task","status":"none","parents":["rule"],"parent_gate":"ready"}
    }})).await?;
    f.write(OWNER, json!({"op":"save_object","kind":"story","id":"one","revision":0,"parent":null,"data":{"title":"One rule","body":"Implement the agreed rule","owner":OWNER},"reason":"Set the rule"})).await?;
    for (_, criterion, _) in PLANS {
        let id = criterion.split_once(':').unwrap().1;
        f.write(OWNER, json!({"op":"save_object","kind":"rule","id":id,"revision":0,"parent":"story:one","data":{"title":id,"owner":OWNER},"reason":"Set the criterion"})).await?;
    }
    let revision = f.read("story:one").await?["process"]["revision"].clone();
    f.write(
        OWNER,
        json!({"op":"confirm","req":"story:one","revision":revision}),
    )
    .await?;
    let mut claims = BTreeMap::new();
    for (name, criterion, task) in PLANS {
        f.write(OWNER, json!({"op":"assess","req":"story:one","revision":revision,"ac":criterion,"assessment":{"ready":true,"reason":"Known scope"}})).await?;
        let rule = f.read(criterion).await?;
        let reply = f
            .write(
                PLANNER,
                json!({"op":"claim_planning","ac":criterion,"revision":rule["current_revision"]}),
            )
            .await?;
        claims.insert(name, json!({"actor":PLANNER,"position":reply["position"]}));
        if let Some(task) = task {
            let id = task.split_once(':').unwrap().1;
            f.write(PLANNER, json!({"op":"save_object","kind":"job","id":id,"revision":0,"parent":criterion,"data":{"title":id,"owner":OWNER,"contract":"Implement the agreed mechanism","purpose":"implementation"},"reason":"Plan the work"})).await?;
        }
    }
    f.write(
        PLANNER,
        json!({"op":"finish_planning","ac":"rule:finished","reason":"Plan complete"}),
    )
    .await?;
    f.write(
        PLANNER,
        json!({"op":"release_planning","ac":"rule:released","reason":"Plan withdrawn"}),
    )
    .await?;

    // Reader R reads the workspace through one bearer token on HTTP and MCP.
    let dir = std::env::temp_dir().join(format!(
        "joppa-planning-restart-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir)?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, json!({ token.clone(): READER }).to_string())?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let read = || async {
        let mut out = BTreeMap::new();
        for (transport, mcp) in [("HTTP", false), ("MCP", true)] {
            out.insert(
                transport,
                planning_snapshot(&http, &root, &f.ws, &token, mcp).await?,
            );
        }
        Ok::<_, anyhow::Error>(out)
    };
    let check = |point: &str, out: &BTreeMap<&str, Snapshot>| {
        for (transport, s) in out {
            expect_plans(point, transport, s, &claims)?;
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
    let after = read().await?;
    drop(server);
    ensure!(
        after == before,
        "the restart changed the plans: before {before:#?}, after {after:#?}"
    );
    check("after the restart", &after)?;
    store::check_journal(&f.pool).await?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

const HOLDER: &str = "worker-p";

/// The two tests that criterion A binds, in the recorded order.
const CHECKS: [&str; 2] = [
    "tests/auth/login.test.ts > login > opens",
    "tests/auth/login.test.ts > login > refuses",
];
