// Part 21 of tests/reports_http.rs, included there, so the test names stay unchanged.

// failed-task-blocks-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_planner_unblocks_a_task_and_finishes_planning_in_one_batch() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.blocked_dependency().await?;
    p.claim("agent", "criterion:a").await?;
    let t0 = p.read("work:t").await?;
    let u0 = p.read("work:u").await?;
    let block = t0["process"]["blocked"].clone();
    let response = p.unblock_and_finish().await?;
    ensure!(response["result"]["isError"] != true, "{response}");

    let t = p.read("work:t").await?;
    let revision = t0["current_revision"].as_u64().unwrap() + 1;
    ensure!(
        t["process"]["blocked"].is_null()
            && t["current_revision"] == revision
            && t["address"] == t0["address"]
            && t["document"]["data"] == t0["document"]["data"]
            && t["document"]["parent"] == t0["document"]["parent"]
            && t["document"]["links"] == t0["document"]["links"]
            && t["process"]["origin_revision"] == t0["process"]["origin_revision"]
            && t["process"]["assignee"].is_null()
            && t["process"]["status"] == "open",
        "T changed beyond its block: {t0} {t}"
    );
    let u = p.read("work:u").await?;
    ensure!(
        u["review_required"] == false
            && u["process"]["review_required"] == false
            && u["document"]["links"] == u0["document"]["links"],
        "U needs review: {u}"
    );
    let a = p.read("criterion:a").await?;
    ensure!(a["process"]["planning_claim"].is_null(), "{a}");

    // T is claimable through HTTP and MCP.
    let root = w.base.split("/api/").next().unwrap();
    let http: Value = w
        .http
        .get(format!("{root}/api/workspaces/{}/tasks?view=all", p.ws))
        .bearer_auth(&w.token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mcp = p.mcp("joppa_tasks", json!({"view":"all"})).await?;
    let mcp: Value = serde_json::from_str(
        mcp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default(),
    )?;
    for (transport, page) in [("HTTP", http), ("MCP", mcp)] {
        let tasks = task_list(&page).as_array().cloned().unwrap_or_default();
        ensure!(
            tasks
                .iter()
                .any(|t| t["address"] == "work:t" && t["claimable"] == true),
            "{transport} all: {page}"
        );
    }

    // The T history keeps the block and records the unblock with P, reason and evidence.
    let read = async |revision: u64| {
        store::read_objects(
            &p.pool,
            &p.ws,
            "owner",
            &store::ObjectScope {
                address: Some("work:t"),
                revision: Some(revision as u32),
                ..Default::default()
            },
        )
        .await
    };
    let blocked = read(revision - 1).await?;
    let unblocked = read(revision).await?;
    ensure!(
        blocked["document"]["position"] == block["position"]
            && blocked["document"]["reason"] == "Verification failed"
            && unblocked["document"]["actor"] == "agent"
            && unblocked["document"]["reason"] == UNBLOCK_REASON,
        "{blocked} {unblocked}"
    );
    let history = store::read(&p.pool, &p.ws, "owner", &store::Scope::full()).await?;
    let event = history["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["position"] == unblocked["document"]["position"])
        .cloned()
        .unwrap_or_default();
    ensure!(
        event["actor"] == "agent"
            && event["command"]["op"] == "unblock_task"
            && event["command"]["reason"] == UNBLOCK_REASON
            && event["command"]["evidence"] == "Run: r1",
        "{event}"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}

// failed-task-blocks-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_refused_finish_rolls_back_the_unblock_in_the_same_batch() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.blocked_dependency().await?;
    // Fixture 2: a second unchanged blocked Task V under A.
    p.save("work:v", Some("criterion:a"), json!({"title":"V","owner":"owner","contract":"Implement this criterion","purpose":"implementation"}), json!([])).await?;
    let revision = p.revision("work:v").await?;
    p.write(
        "other",
        json!({"op":"claim_task","id":"work:v","revision":revision}),
    )
    .await?;
    p.write("other", json!({"op":"block_task","id":"work:v","revision":revision + 1,"reason":"Verification failed","evidence":"test y failed on abc123"})).await?;
    p.claim("agent", "criterion:a").await?;
    let before = p.saved().await?;
    let response = p.unblock_and_finish().await?;
    ensure!(
        response["result"]["isError"] == true
            && response["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|e| e.contains(NEEDS_CONTRACT)),
        "{response}"
    );
    ensure!(
        p.saved().await? == before,
        "the refused batch changed saved state"
    );
    let a = p.read("criterion:a").await?;
    let t = p.read("work:t").await?;
    ensure!(
        a["process"]["planning_claim"]["actor"] == "agent" && !t["process"]["blocked"].is_null(),
        "{a} {t}"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}

// failed-task-blocks-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_workspace_owner_unblocks_without_a_claim_and_the_journal_replays() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.blocked_dependency().await?;
    let revision = p.revision("work:t").await?;
    p.write("owner", unblock_t(revision)).await?;
    let t = p.read("work:t").await?;
    ensure!(
        t["process"]["blocked"].is_null() && t["current_revision"] == revision + 1,
        "{t}"
    );
    let before = p.saved().await?;
    let refused = p.write("owner", unblock_t(revision + 1)).await;
    ensure!(
        refused
            .as_ref()
            .is_err_and(|e| e.to_string() == "Task is not blocked"),
        "{refused:?}"
    );
    ensure!(
        p.saved().await? == before,
        "the refusal changed saved state"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}

const NO_TASK_CHANGES: &str = "planning produced no Task changes";

impl Planning<'_> {
    /// AC6 fixture: A is ready and grey with X listed; Y passes in a record. P claims A.
    async fn bound_plan(&self) -> Result<()> {
        // check-list-frozen-ac3: a list save clears readiness, so A is assessed after it.
        self.write(
            "agent",
            json!({"op":"set_ac_tests","target":"criterion:a","tests":[A],"reason":"Link the tests"}),
        )
        .await?;
        self.ready().await?;
        self.record("c1c1c1c", outcomes(&[("refuses", "pass")]))
            .await?;
        let status = self.tests().await?;
        ensure!(
            status["status"] == "grey" && listed(&status) == [A],
            "{status}"
        );
        self.claim("agent", "criterion:a").await
    }
    /// GET /tests?ac=criterion:a.
    async fn tests(&self) -> Result<Value> {
        let root = self.w.base.split("/api/").next().unwrap();
        Ok(self
            .w
            .http
            .get(format!(
                "{root}/api/workspaces/{}/tests?ac=criterion:a",
                self.ws
            ))
            .bearer_auth(&self.w.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    /// Addresses of the saved objects.
    async fn addresses(&self) -> Result<Vec<Value>> {
        Ok(self.saved().await?["objects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["address"].clone())
            .collect())
    }
}

fn save_xy() -> Value {
    json!({"op":"set_ac_tests","target":"criterion:a","tests":[A,B],"reason":"Bind the existing tests"})
}
