// Part 19 of tests/reports_http.rs, included there, so the test names stay unchanged.

// failed-task-blocks-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_blocked_task_refuses_claims_and_leaves_the_queue_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    let position = p.blocked("work:t").await?;
    ensure!(p.saved().await?["position"] == position);
    let block = json!({"actor":"other","reason":"Verification failed","evidence":"test x failed on abc123","position":position});
    let t = p.read("work:t").await?;
    ensure!(
        t["current_revision"] == 5
            && t["process"]["status"] == "open"
            && t["process"]["assignee"].is_null()
            && t["process"]["blocked"] == block,
        "{t}"
    );
    ensure!(p.saved_object("work:t").await?["process"]["blocked"] == block);

    p.refuse_command(
        "claim_task on blocked T",
        json!({"op":"claim_task","id":"work:t","revision":5}),
        BLOCKED,
    )
    .await?;
    p.refuse_command(
        "claim_next_task with only blocked T",
        json!({"op":"claim_next_task"}),
        BLOCKED,
    )
    .await?;

    let root = w.base.split("/api/").next().unwrap();
    for view in ["available", "all"] {
        let http: Value = w
            .http
            .get(format!("{root}/api/workspaces/{}/tasks?view={view}", p.ws))
            .bearer_auth(&w.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mcp = p.mcp("joppa_tasks", json!({"view":view})).await?;
        let mcp: Value = serde_json::from_str(
            mcp["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_default(),
        )?;
        for (transport, page) in [("HTTP", http), ("MCP", mcp)] {
            let tasks = task_list(&page).as_array().cloned().unwrap_or_default();
            let t = tasks.iter().find(|t| t["address"] == "work:t");
            if view == "available" {
                ensure!(t.is_none(), "{transport} available lists T: {page}");
            } else {
                ensure!(
                    t.is_some_and(|t| t["claimable"] == false && t["waiting_reason"] == BLOCKED),
                    "{transport} all: {page}"
                );
            }
        }
    }

    // The HTTP envelope keeps its checks; the raw command meets the shared validation.
    let before = p.saved().await?;
    let valid = json!({"op":"save_object","kind":"spec","id":"envelope","revision":0,"parent":null,"data":{"title":"Envelope","body":"Check the envelope","owner":"owner"},"reason":"Check the envelope"});
    let id = || json!(uuid::Uuid::new_v4().to_string());
    let mut with_actor = valid.clone();
    with_actor["actor"] = json!("other");
    let mut unknown_field = valid.clone();
    unknown_field["extra"] = json!(true);
    let mut missing_field = valid.clone();
    missing_field.as_object_mut().unwrap().remove("reason");
    for (case, body) in [
        ("missing request_id", json!({"command":valid})),
        ("missing command", json!({"request_id":id()})),
        (
            "unknown envelope field",
            json!({"request_id":id(),"command":valid,"extra":true}),
        ),
        (
            "envelope actor",
            json!({"request_id":id(),"command":valid,"actor":"other"}),
        ),
        (
            "command actor",
            json!({"request_id":id(),"command":with_actor}),
        ),
        (
            "unknown command field",
            json!({"request_id":id(),"command":unknown_field}),
        ),
        (
            "missing command field",
            json!({"request_id":id(),"command":missing_field}),
        ),
        (
            "unknown operation",
            json!({"request_id":id(),"command":{"op":"made_up_task_operation","id":"work:t","revision":5}}),
        ),
        (
            "command is not an object",
            json!({"request_id":id(),"command":"claim_next_task"}),
        ),
    ] {
        let (code, reply) = p.http(body).await?;
        ensure!((400..500).contains(&code), "{case}: {code} {reply}");
        ensure!(
            p.saved().await? == before,
            "{case}: the refusal changed saved state"
        );
    }
    let (code, reply) = p.http(json!({"request_id":id(),"command":valid})).await?;
    ensure!(code == 200, "the valid envelope: {code} {reply}");
    store::check_journal(&p.pool).await?;
    Ok(())
}

// failed-task-blocks-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn claim_next_task_takes_the_ordinary_task_beside_a_blocked_one() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        let position = p.blocked("work:t").await?;
        p.save("work:ordinary", Some("criterion:a"), json!({"title":"Ordinary","owner":"owner","contract":"Implement this criterion","purpose":"implementation"}), json!([])).await?;
        let reply = p
            .send(mcp, &json!({"op":"claim_next_task"}))
            .await?
            .map_err(anyhow::Error::msg)?;
        let ordinary = p.read("work:ordinary").await?;
        ensure!(
            ordinary["process"]["assignee"] == "agent",
            "mcp={mcp}: {reply} {ordinary}"
        );
        let t = p.read("work:t").await?;
        ensure!(
            t["current_revision"] == 5
                && t["process"]["assignee"].is_null()
                && t["process"]["blocked"]["position"] == position,
            "mcp={mcp}: {t}"
        );
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// failed-task-blocks-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn claim_next_task_keeps_the_no_match_error_for_held_and_unpublished_tasks() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    let revision = p.revision("work:t0").await?;
    p.write(
        "other",
        json!({"op":"claim_task","id":"work:t0","revision":revision}),
    )
    .await?;
    p.refuse_command(
        "only T0, held by Q",
        json!({"op":"claim_next_task"}),
        NO_MATCH,
    )
    .await?;

    let p = planning(&w).await?;
    p.ready().await?;
    p.claim("other", "criterion:a").await?;
    p.write("other", json!({"op":"save_object","kind":"work","id":"new","revision":0,"parent":"criterion:a","data":{"title":"New","owner":"owner","contract":"Implement this criterion","purpose":"implementation"},"reason":"Plan the work"})).await?;
    p.refuse_command(
        "only an unpublished Task",
        json!({"op":"claim_next_task"}),
        NO_MATCH,
    )
    .await?;
    store::check_journal(&p.pool).await?;
    Ok(())
}

const NEEDS_CONTRACT: &str = "blocked Task needs a new contract or retirement";

const OWNER_REQUIRED: &str = "active Task owner required";

impl Planning<'_> {
    /// Fixture 1 of failed-task-blocks-ac3: L blocks T under ready A, then P claims A.
    /// Returns the block of T.
    async fn blocked_plan(&self) -> Result<Value> {
        let position = self.blocked("work:t").await?;
        self.claim("agent", "criterion:a").await?;
        Ok(
            json!({"actor":"other","reason":"Verification failed","evidence":"test x failed on abc123","position":position}),
        )
    }
    /// P saves `key` under A with `contract` and `links`.
    async fn plan_task(&self, key: &str, contract: &str, links: Value) -> Result<()> {
        let (kind, id) = key.split_once(':').unwrap();
        let revision = self.revision(key).await.unwrap_or(0);
        self.write("agent", json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":"criterion:a","data":{"title":"T","owner":"owner","contract":contract,"purpose":"implementation"},"links":links,"reason":"Plan the work"})).await?;
        Ok(())
    }
    /// P finishes A through HTTP or MCP.
    async fn finish(&self, mcp: bool) -> Result<()> {
        self.send(
            mcp,
            &json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
        Ok(())
    }
}

fn finish_command() -> Value {
    json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"})
}
