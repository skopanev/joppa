// Part 20 of tests/reports_http.rs, included there, so the test names stay unchanged.

// failed-task-blocks-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn finish_planning_needs_a_new_contract_for_a_blocked_task_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        let block = p.blocked_plan().await?;
        // Fixture 3 and its controls: an identical save and an unrelated new Task.
        p.refuse_command("unchanged blocked T", finish_command(), NEEDS_CONTRACT)
            .await?;
        p.plan_task("work:t", "Implement this criterion", json!([]))
            .await?;
        p.refuse_command("identical save of T", finish_command(), NEEDS_CONTRACT)
            .await?;
        p.plan_task("work:new", "Implement other work", json!([]))
            .await?;
        p.refuse_command("only an unrelated Task", finish_command(), NEEDS_CONTRACT)
            .await?;
        let a = p.read("criterion:a").await?;
        let t = p.read("work:t").await?;
        ensure!(
            a["process"]["planning_claim"]["actor"] == "agent" && t["process"]["blocked"] == block,
            "{a} {t}"
        );

        // Fixture 1: P amends the T contract text and finishes.
        p.amend(
            "agent",
            "work:t",
            "T",
            "Implement the criterion with a new fix",
        )
        .await?;
        p.finish(mcp).await?;
        let a = p.read("criterion:a").await?;
        let t = p.read("work:t").await?;
        ensure!(
            a["process"]["planning_claim"].is_null() && t["process"]["blocked"].is_null(),
            "mcp={mcp}: {a} {t}"
        );
        ensure!(p.saved_object("work:t").await?["process"]["blocked"].is_null());
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// failed-task-blocks-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn finish_planning_clears_a_block_after_a_new_dependency_and_names_it() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.blocked_plan().await?;
    // Fixture 2: P adds Task C under A and a blocking link from T to C.
    p.plan_task("work:c", "Deliver the prerequisite", json!([]))
        .await?;
    p.plan_task(
        "work:t",
        "Implement this criterion",
        json!([{"target":"work:c","relation":"depends_on","revision":1,"blocking":true}]),
    )
    .await?;
    p.finish(false).await?;
    let a = p.read("criterion:a").await?;
    let t = p.read("work:t").await?;
    ensure!(
        a["process"]["planning_claim"].is_null() && t["process"]["blocked"].is_null(),
        "{a} {t}"
    );
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
        let t = tasks.iter().find(|t| t["address"] == "work:t");
        ensure!(
            // MCP omits a false claimable.
            t.is_some_and(|t| t["claimable"] != true
                && t["waiting_reason"]
                    .as_str()
                    .is_some_and(|r| r.contains("work:c") && r != BLOCKED)),
            "{transport} all: {page}"
        );
    }
    store::check_journal(&p.pool).await?;
    Ok(())
}

// failed-task-blocks-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_planner_retires_a_blocked_task_and_other_actors_cannot_over_http_and_mcp() -> Result<()>
{
    let w = world().await?;
    let p = planning(&w).await?;
    let position = p.blocked("work:t").await?;
    p.save("work:u", Some("criterion:a"), json!({"title":"U","owner":"owner","body":"Implement this criterion","purpose":"implementation"}), json!([])).await?;
    let retire = |key: &str, revision: u64| json!({"op":"retire_task","id":key,"revision":revision,"reason":"The planned work replaces this Task"});
    // P is not yet the planner, the owner or the workspace owner.
    let revision = p.revision("work:t").await?;
    p.refuse_command(
        "P retires blocked T without a claim",
        retire("work:t", revision),
        OWNER_REQUIRED,
    )
    .await?;
    let before = p.saved().await?;
    ensure!(p.write("other", retire("work:t", revision)).await.is_err());
    ensure!(p.saved().await? == before, "Q changed saved state");
    p.claim("agent", "criterion:a").await?;
    let u = p.revision("work:u").await?;
    p.refuse_command("P retires unblocked U", retire("work:u", u), OWNER_REQUIRED)
        .await?;

    // Fixture 4: P retires blocked T and finishes.
    p.send(true, &retire("work:t", revision))
        .await?
        .map_err(anyhow::Error::msg)?;
    p.finish(false).await?;
    let a = p.read("criterion:a").await?;
    let t = p.read("work:t").await?;
    ensure!(
        a["process"]["planning_claim"].is_null() && t["process"]["status"] == "retired",
        "{a} {t}"
    );
    let block = store::read_objects(
        &p.pool,
        &p.ws,
        "owner",
        &store::ObjectScope {
            address: Some("work:t"),
            revision: Some(5),
            ..Default::default()
        },
    )
    .await?;
    ensure!(
        block["document"]["position"] == position
            && block["document"]["reason"] == "Verification failed",
        "{block}"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}

const UNBLOCK_REASON: &str = "Startup failure; checkout discarded";

const PLANNER_OR_OWNER: &str = "planning holder or workspace owner required";

impl Planning<'_> {
    /// Fixture 1 of failed-task-blocks-ac5: L blocks T under A, and U under B
    /// depends on T with a blocking link pinned to the current T revision.
    async fn blocked_dependency(&self) -> Result<()> {
        self.blocked("work:t").await?;
        let pin = self.revision("work:t").await?;
        self.save("work:u", Some("criterion:b"), json!({"title":"U","owner":"owner","body":"Implement this criterion","purpose":"implementation"}),
            json!([{"target":"work:t","relation":"depends_on","revision":pin,"blocking":true}])).await
    }
    /// P sends unblock_task for T and finish_planning for A in one MCP batch.
    async fn unblock_and_finish(&self) -> Result<Value> {
        let revision = self.revision("work:t").await?;
        self.mcp(
            "joppa_batch",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"operations":[
                {"command":unblock_t(revision)},{"command":finish_command()}
            ]}),
        )
        .await
    }
}

fn unblock_t(revision: u64) -> Value {
    json!({"op":"unblock_task","id":"work:t","revision":revision,"reason":UNBLOCK_REASON,"evidence":"Run: r1"})
}

// failed-task-blocks-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn unblock_task_refuses_a_member_and_a_stale_revision_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    // Q holds the planning claim on A. The token member is M without a claim.
    let p = planning(&w).await?;
    p.blocked_dependency().await?;
    p.claim("other", "criterion:a").await?;
    let revision = p.revision("work:t").await?;
    p.refuse_command("M unblocks T", unblock_t(revision), PLANNER_OR_OWNER)
        .await?;
    // The token member is P with the planning claim on A.
    let p = planning(&w).await?;
    p.blocked_dependency().await?;
    p.claim("agent", "criterion:a").await?;
    let revision = p.revision("work:t").await?;
    p.refuse_command(
        "P unblocks T at a stale revision",
        unblock_t(revision - 1),
        "stale Task revision; read the Task again",
    )
    .await?;
    store::check_journal(&p.pool).await?;
    Ok(())
}
