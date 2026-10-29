// Part 25 of tests/reports_http.rs, included there, so the test names stay unchanged.

// planning-claim-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac3_a_valid_creation_plan_publishes_its_tasks_and_each_refusal_keeps_the_claim()
-> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.held_t0().await?;
        p.plan_task("work:t1", "Implement this criterion", json!([]))
            .await?;
        p.plan_task(
            "work:t2",
            "Implement the dependent work",
            json!([{"target":"work:t1","relation":"depends_on","revision":1,"blocking":true}]),
        )
        .await?;
        let verification = p.read("criterion:a").await?["process"]["verification"].clone();
        let new = ["work:t1", "work:t2"];
        p.refuse_command_as(&w.other, "Q finishes A", finish_command(), HOLDER_REQUIRED)
            .await?;
        p.unpublished(&new).await?;
        let blank = json!({"op":"finish_planning","ac":"criterion:a","reason":" "});
        p.refuse_command("blank reason", blank, EMPTY_TEXT).await?;
        p.unpublished(&new).await?;

        p.finish(mcp).await?;
        for (transport, claim) in p.planning_claims("criterion:a").await? {
            ensure!(claim.is_null(), "mcp={mcp} {transport}: {claim}");
        }
        ensure!(p.read("criterion:a").await?["process"]["verification"] == verification);
        for (transport, t0) in p.object(&w.token, "work:t0").await? {
            ensure!(
                t0["process"]["status"] == "in_progress" && t0["process"]["assignee"] == "worker",
                "{transport}: {t0}"
            );
        }
        for (transport, t) in p.entry(&w.token, "work:t1").await? {
            ensure!(
                t["claimable"] == true && t["waiting_reason"].is_null(),
                "{transport}: {t}"
            );
        }
        for (transport, t) in p.entry(&w.token, "work:t2").await? {
            ensure!(
                t["claimable"] == false
                    && t["waiting_reason"]
                        == "Task has an unmet dependency or a changed dependency revision: work:t1",
                "{transport}: {t}"
            );
        }
        for (transport, page) in p.view(&w.token, "available").await? {
            ensure!(
                queued(&page, "work:t1").is_some() && queued(&page, "work:t2").is_none(),
                "{transport} available: {page}"
            );
        }
        p.refuse_command("no active claim", finish_command(), NO_CLAIM)
            .await?;
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// planning-claim-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac3_an_update_only_plan_finishes_and_a_plan_without_changes_is_refused() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.held_t0().await?;
        p.refuse_command("no holder changes", finish_command(), NO_TASK_CHANGES)
            .await?;
        for (transport, claim) in p.planning_claims("criterion:a").await? {
            ensure!(claim["actor"] == "agent", "{transport}: {claim}");
        }
        let verification = p.read("criterion:a").await?["process"]["verification"].clone();
        p.update_t0().await?;
        p.finish(mcp).await?;
        for (transport, claim) in p.planning_claims("criterion:a").await? {
            ensure!(claim.is_null(), "mcp={mcp} {transport}: {claim}");
        }
        ensure!(p.read("criterion:a").await?["process"]["verification"] == verification);
        for (transport, t0) in p.object(&w.token, "work:t0").await? {
            ensure!(
                t0["process"]["status"] == "in_progress"
                    && t0["process"]["assignee"] == "worker"
                    && t0["document"]["data"]["contract"] == "Implement the updated plan",
                "{transport}: {t0}"
            );
        }
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// planning-claim-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac3_a_plan_that_depends_on_an_archived_criterion_is_refused() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    let revision = p.prerequisite("Deliver the prerequisite").await?;
    p.ready().await?;
    p.claim("agent", "criterion:a").await?;
    p.plan_task(
        "work:t1",
        "Implement this criterion",
        json!([{"target":"criterion:c","relation":"depends_on","revision":revision,"blocking":true}]),
    )
    .await?;
    // The owner archives C in spec:two after P links the new Task to it.
    p.write("owner", json!({"op":"archive_object","target":"criterion:c","revision":revision,"reason":"Drop the prerequisite"})).await?;
    ensure!(p.read("criterion:c").await?["document"]["present"] == false);
    p.refuse_command(
        "archived dependency target",
        finish_command(),
        "planning dependency target is missing",
    )
    .await?;
    p.unpublished(&["work:t1"]).await?;
    store::check_journal(&p.pool).await?;
    Ok(())
}

// planning-claim-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac4_a_holder_release_retires_only_the_new_tasks_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let fields = |t: &Value| {
        json!([
            t["process"]["status"],
            t["process"]["assignee"],
            t["current_revision"],
            t["document"]
        ])
    };
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.held_t0().await?;
        for task in ["work:t1", "work:t2"] {
            p.plan_task(task, "Implement this criterion", json!([]))
                .await?;
        }
        p.update_t0().await?;
        let t0 = p.object(&w.token, "work:t0").await?;
        let history = p.history("work:t0").await?;
        for (transport, t) in &t0 {
            ensure!(
                t["process"]["status"] == "in_progress"
                    && t["process"]["assignee"] == "worker"
                    && t["document"]["actor"] == "agent"
                    && history.len() as u64 == t["current_revision"].as_u64().unwrap(),
                "{transport}: {t}"
            );
        }

        p.send(mcp, &release_command("Plan withdrawn"))
            .await?
            .map_err(anyhow::Error::msg)?;
        for (transport, claim) in p.planning_claims("criterion:a").await? {
            ensure!(claim.is_null(), "mcp={mcp} {transport}: {claim}");
        }
        for task in ["work:t1", "work:t2"] {
            for (transport, t) in p.object(&w.token, task).await? {
                ensure!(
                    t["process"]["status"] == "retired"
                        && t["process"]["retired"] == "Plan withdrawn"
                        && t["process"]["assignee"].is_null()
                        && t["document"]["present"] == true,
                    "{transport} {task}: {t}"
                );
            }
        }
        for ((transport, old), (_, new)) in t0.iter().zip(p.object(&w.token, "work:t0").await?) {
            ensure!(fields(old) == fields(&new), "{transport}: {old} {new}");
        }
        ensure!(p.history("work:t0").await? == history, "T0 history changed");
        let saved = p.saved().await?;
        let event = saved["events"]
            .as_array()
            .unwrap()
            .iter()
            .max_by_key(|e| e["position"].as_i64())
            .cloned()
            .unwrap_or_default();
        ensure!(
            event["actor"] == "agent"
                && event["command"]["op"] == "release_planning"
                && event["command"]["reason"] == "Plan withdrawn",
            "{event}"
        );
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// planning-claim-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac4_an_empty_release_succeeds_and_each_release_refusal_changes_nothing() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.ready().await?;
        p.claim("agent", "criterion:a").await?;
        p.refuse_command_as(
            &w.other,
            "Q releases A",
            release_command("Plan withdrawn"),
            "workspace owner required",
        )
        .await?;
        p.refuse_command("blank reason", release_command(" "), EMPTY_TEXT)
            .await?;
        let objects = p.addresses().await?;
        p.send(mcp, &release_command("Plan withdrawn"))
            .await?
            .map_err(anyhow::Error::msg)?;
        for (transport, claim) in p.planning_claims("criterion:a").await? {
            ensure!(claim.is_null(), "mcp={mcp} {transport}: {claim}");
        }
        ensure!(p.addresses().await? == objects, "release changed objects");
        p.refuse_command(
            "no active claim",
            release_command("Plan withdrawn"),
            NO_CLAIM,
        )
        .await?;
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}
