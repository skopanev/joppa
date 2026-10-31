// Part 31 of tests/reports_http.rs, included there, so the test names stay unchanged.

// dependencies-before-priority-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac2_a_task_under_waiting_work_cannot_be_claimed_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1"]).await?;
    p.link("spec:b", ordering_link("spec:a")).await?;
    p.save("work:t", Some("criterion:b1"), json!({"title":"T","owner":"owner","body":"Implement this criterion","purpose":"implementation"}), json!([]))
        .await?;
    ensure!(p.tasks_both("mine").await?.is_empty());
    let reason = joppa::dependencies::reason("spec:a");
    let revision = p.revision("work:t").await?;
    p.refuse_command(
        "claim_task on T",
        json!({"op":"claim_task","id":"work:t","revision":revision}),
        &reason,
    )
    .await?;
    p.refuse_command(
        "claim_next_task with only T",
        json!({"op":"claim_next_task"}),
        &reason,
    )
    .await?;
    let t = p.object_both("work:t").await?;
    ensure!(
        t["process"]["assignee"].is_null() && t["process"]["status"] == "open",
        "{t}"
    );
    let listed = |tasks: &[Value]| tasks.iter().find(|t| t["address"] == "work:t").cloned();
    ensure!(listed(&p.tasks_both("available").await?).is_none());
    for view in ["all", "waiting"] {
        let t = listed(&p.tasks_both(view).await?);
        ensure!(
            t.as_ref()
                .is_some_and(|t| t["claimable"] == false && t["waiting_reason"] == reason.as_str()),
            "view={view}: {t:?}"
        );
    }
    // Release: a record makes every criterion of A green.
    p.write(
        "agent",
        json!({"op":"set_ac_tests","target":"criterion:a1","tests":[A],"reason":"Bind the test"}),
    )
    .await?;
    p.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let t = listed(&p.tasks_both("available").await?);
    ensure!(
        t.is_some_and(|t| t["claimable"] == true),
        "available after A is green"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}

// dependencies-before-priority-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac3_a_shared_parent_alone_never_makes_work_wait_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1"]).await?;
    // Controls: A1 and B1 are grey, and B1 is ready.
    ensure!(p.status("criterion:a1").await?["status"] == "grey");
    ensure!(p.status("criterion:b1").await?["status"] == "grey");
    let readiness = p.readiness("criterion:b1").await?;
    ensure!(readiness[0] == "ready", "{readiness}");
    let free =
        |b1: &Value| b1["work_ready"] == true && b1["process"]["readiness_blocker"].is_null();
    ensure!(p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    let b1 = p.object_both("criterion:b1").await?;
    ensure!(free(&b1), "{b1}");
    // Control: a blocking link from B to A makes B1 wait until it is removed.
    p.link("spec:b", ordering_link("spec:a")).await?;
    ensure!(!p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    p.link("spec:b", json!([])).await?;
    ensure!(p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    let b1 = p.object_both("criterion:b1").await?;
    ensure!(free(&b1), "{b1}");
    ensure!(p.readiness("criterion:b1").await? == readiness);
    Ok(())
}

// dependencies-before-priority-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac4_a_red_target_criterion_keeps_dependents_waiting_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1", "criterion:a2"]).await?;
    for (ac, test) in [("criterion:a1", A), ("criterion:a2", B)] {
        p.write(
            "agent",
            json!({"op":"set_ac_tests","target":ac,"tests":[test],"reason":"Bind the test"}),
        )
        .await?;
    }
    p.link("spec:b", ordering_link("spec:a")).await?;
    p.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    // Controls: A1 and A2 are green, B1 is ready and grey, and B1 is listed.
    for ac in ["criterion:a1", "criterion:a2"] {
        ensure!(p.status(ac).await?["status"] == "green", "{ac}");
    }
    ensure!(p.status("criterion:b1").await?["status"] == "grey");
    let readiness = p.readiness("criterion:b1").await?;
    ensure!(readiness[0] == "ready", "{readiness}");
    ensure!(p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    // A record fails the test of A2.
    p.record(
        "c2c2c2c",
        outcomes(&[("opens", "pass"), ("refuses", "fail")]),
    )
    .await?;
    ensure!(p.status("criterion:a2").await?["status"] == "red");
    ensure!(!p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    let b1 = p.object_both("criterion:b1").await?;
    ensure!(waits_for(&b1, "spec:a"), "{b1}");
    // A later record passes every listed test of A.
    p.record(
        "c3c3c3c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(p.status("criterion:a2").await?["status"] == "green");
    ensure!(p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    Ok(())
}

// dependencies-before-priority-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac6_a_dependency_cycle_through_the_tree_is_refused_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1"]).await?;
    p.link("spec:b", ordering_link("spec:a")).await?;
    let save = |key: &str, object: &Value, links: Value| {
        let (kind, id) = key.split_once(':').unwrap();
        json!({"op":"save_object","kind":kind,"id":id,"revision":object["current_revision"],"parent":object["document"]["parent"],
            "data":object["document"]["data"],"links":links,"reason":"Order the work"})
    };
    // Each refusal leaves the committed rows and the journal position unchanged.
    for (case, source, target) in [
        ("fixture 1: A1 waits for B1", "criterion:a1", "criterion:b1"),
        ("fixture 2: A waits for B", "spec:a", "spec:b"),
    ] {
        let object = p.read(source).await?;
        p.refuse_command_as(
            &w.owner,
            case,
            save(source, &object, ordering_link(target)),
            "dependency cycle",
        )
        .await?;
        ensure!(
            p.object_both(source).await?["document"]["links"] == json!([]),
            "{case}"
        );
    }
    // Controls: a related link, a blocking link to an unrelated requirement and the removal
    // of the link from B to A make no cycle.
    p.save(
        "spec:d",
        Some("cap:c"),
        json!({"title":"spec:d","body":"Deliver another rule","owner":"owner"}),
        json!([]),
    )
    .await?;
    p.link("spec:a", json!([{"target":"spec:b","relation":"related"}]))
        .await?;
    p.link("criterion:b1", ordering_link("spec:d")).await?;
    p.link("spec:b", json!([])).await?;
    ensure!(p.read("criterion:b1").await?["document"]["links"] == ordering_link("spec:d"));
    ensure!(p.read("spec:b").await?["document"]["links"] == json!([]));
    store::check_journal(&p.pool).await?;
    Ok(())
}
