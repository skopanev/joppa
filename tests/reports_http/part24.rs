// Part 24 of tests/reports_http.rs, included there, so the test names stay unchanged.

// planning-claim-ac7
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac7_finish_keeps_the_wait_for_a_changed_blocking_dependency() -> Result<()> {
    let w = world().await?;
    let dependency = |contract: &str| json!({"title":"D","owner":"owner","contract":contract,"purpose":"implementation"});
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.save(
            "work:d",
            Some("criterion:b"),
            dependency("Deliver the dependency"),
            json!([]),
        )
        .await?;
        p.replanned(
            json!([{"target":"work:d","relation":"depends_on","revision":1,"blocking":true}]),
        )
        .await?;
        p.save(
            "work:d",
            Some("criterion:b"),
            dependency("Deliver the changed dependency"),
            json!([]),
        )
        .await?;
        let before = p.object(&w.token, "work:t").await?;
        for (transport, t) in &before {
            ensure!(t["review_required"] == true, "{transport}: {t}");
        }
        p.finish(mcp).await?;
        p.expect_t("after finish", |t| {
            t["review_required"] == false
                && t["claimable"] == false
                && t["waiting_reason"]
                    == "Task has an unmet dependency or a changed dependency revision: work:d"
                && status_label(t) == Some("Dependency revision changed")
        })
        .await?;
        // Finish keeps the dependency pin and the dependency review.
        let after = p.object(&w.token, "work:t").await?;
        for ((transport, old), (_, new)) in before.iter().zip(&after) {
            ensure!(
                new["document"]["links"] == old["document"]["links"]
                    && new["review_required"] == true
                    && status_label(new) == Some("Dependency revision changed"),
                "{transport}: {old} {new}"
            );
        }
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// planning-claim-ac7
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac7_a_refused_finish_and_a_release_keep_the_old_criterion_pin() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.replanned(json!([])).await?;
    // The refusal compares every saved row, the journal position included.
    p.refuse_command_as(&w.other, "Q finishes A", finish_command(), HOLDER_REQUIRED)
        .await?;
    p.expect_t("after the refused finish", |t| t["review_required"] == true)
        .await?;
    for (transport, claim) in p.planning_claims("criterion:a").await? {
        ensure!(claim["actor"] == "agent", "{transport}: {claim}");
    }
    p.send(false, &release_command("Plan withdrawn"))
        .await?
        .map_err(anyhow::Error::msg)?;
    p.expect_t("after the release", |t| t["review_required"] == true)
        .await?;
    store::check_journal(&p.pool).await?;
    Ok(())
}

// planning-claim-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac2_planning_hides_only_the_new_tasks_of_its_holder_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.ready().await?;
        p.t0().await?;
        let revision = p.revision("criterion:a").await?;
        let claim = p
            .position(
                "agent",
                json!({"op":"claim_planning","ac":"criterion:a","revision":revision}),
            )
            .await?;
        let mut created = vec![];
        for id in ["t1", "t2"] {
            let position = p.position("agent", json!({"op":"save_object","kind":"work","id":id,"revision":0,"parent":"criterion:a","data":{"title":id,"owner":"owner","contract":"Implement this criterion","purpose":"implementation"},"reason":"Plan the work"})).await?;
            created.push((format!("work:{id}"), position));
        }
        ensure!(claim < created[0].1 && created[0].1 < created[1].1);

        // Q reads all and available Tasks.
        for view in ["all", "available"] {
            for (transport, page) in p.view(&w.other, view).await? {
                for (task, _) in &created {
                    let t = queued(&page, task);
                    if view == "available" {
                        ensure!(t.is_none(), "{transport} available lists {task}: {page}");
                    } else {
                        ensure!(
                            t.as_ref().is_some_and(|t| t["status"] == "open"
                                && t["assignee"].is_null()
                                && t["claimable"] == false
                                && t["waiting_reason"] == AWAITING),
                            "{transport} all {task}: {page}"
                        );
                    }
                }
                ensure!(
                    queued(&page, "work:t0").is_some_and(|t| t["claimable"] == true),
                    "{transport} {view} T0: {page}"
                );
            }
        }
        for (task, _) in &created {
            let revision = p.revision(task).await?;
            p.refuse_command_as(
                &w.other,
                &format!("Q claims {task}"),
                json!({"op":"claim_task","id":task,"revision":revision}),
                AWAITING,
            )
            .await?;
        }

        // claim_next_task selects T0, and then no Task is left.
        p.send_as(&w.other, mcp, &json!({"op":"claim_next_task"}))
            .await?
            .map_err(anyhow::Error::msg)?;
        for (transport, t0) in p.object(&w.other, "work:t0").await? {
            ensure!(
                t0["process"]["status"] == "in_progress" && t0["process"]["assignee"] == "other",
                "mcp={mcp} {transport}: {t0}"
            );
        }
        p.refuse_command_as(
            &w.other,
            "claim_next_task with only T1 and T2",
            json!({"op":"claim_next_task"}),
            NO_MATCH,
        )
        .await?;

        for (task, position) in &created {
            for (transport, t) in p.object(&w.other, task).await? {
                ensure!(
                    t["process"]["status"] == "open"
                        && t["process"]["assignee"].is_null()
                        && t["process"]["created_by"] == "agent"
                        && t["process"]["created_at"] == *position
                        && t["document"]["parent"] == "criterion:a",
                    "{transport} {task}: {t}"
                );
            }
        }
        for (transport, planning_claim) in p.planning_claims("criterion:a").await? {
            ensure!(
                planning_claim == json!({"actor":"agent","position":claim}),
                "{transport}: {planning_claim}"
            );
        }
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}
