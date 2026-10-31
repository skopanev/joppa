// Part 33 of tests/reports_http.rs, included there, so the test names stay unchanged.

// pins-ignore-process-events-ac3
// Task B blocks on criterion C, and C lists test A. Records make C green, then red.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_green_criterion_frees_a_blocked_task_and_a_red_one_makes_it_wait() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.ready().await?;
    let pin = p.prerequisite("Deliver the prerequisite").await?;
    p.write(
        "agent",
        json!({"op":"set_ac_tests","target":"criterion:c","tests":[A],"reason":"Bind the test"}),
    )
    .await?;
    // The new test list resets the readiness of C, so the owner confirms C ready again.
    p.confirm(
        "spec:two",
        &[("criterion:c", json!({"ready":true,"reason":"Known scope"}))],
    )
    .await?;
    // The list save gives C a revision that changes only its tests, so the pin stays current.
    ensure!(p.revision("criterion:c").await? == pin + 1);
    p.save("work:b", Some("criterion:a"), json!({"title":"B","owner":"owner","body":"Use the prerequisite","purpose":"implementation"}),
        json!([{"target":"criterion:c","relation":"depends_on","revision":pin,"blocking":true}])).await?;
    // Control: B waits while C is grey.
    ensure!(p.status("criterion:c").await?["status"] == "grey");
    ensure!(!task_addresses(&p.tasks_both("available").await?).contains(&"work:b"));
    ensure!(task_addresses(&p.tasks_both("waiting").await?).contains(&"work:b"));
    p.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let c = p.status("criterion:c").await?;
    ensure!(c["status"] == "green", "C after the passing run: {c}");
    let available = p.tasks_both("available").await?;
    ensure!(
        task_addresses(&available).contains(&"work:b"),
        "B must be available: {available:?}"
    );
    let b = p.object_both("work:b").await?;
    ensure!(
        b["review_required"] == false && b["process"]["assignee"].is_null(),
        "B while C is green: {b}"
    );
    // Fixture 2: a later record fails test A.
    p.record(
        "c2c2c2c",
        outcomes(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    let c = p.status("criterion:c").await?;
    ensure!(c["status"] == "red", "C after the failing run: {c}");
    let available = p.tasks_both("available").await?;
    ensure!(
        !task_addresses(&available).contains(&"work:b"),
        "B must leave the available list: {available:?}"
    );
    let waiting = p.tasks_both("waiting").await?;
    ensure!(
        task_addresses(&waiting).contains(&"work:b"),
        "B must wait: {waiting:?}"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}

// pins-ignore-process-events-ac2
// Task B depends on open Task A. Planner P saves a new contract of A over HTTP, then over MCP.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_contract_of_a_prerequisite_task_makes_its_dependent_wait_for_review() -> Result<()> {
    let w = world().await?;
    let contract = |title: &str, contract: &str| json!({"title":title,"owner":"owner","body":contract,"purpose":"implementation"});
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.ready().await?;
        p.save(
            "work:a",
            Some("criterion:a"),
            contract("A", "Deliver the prerequisite"),
            json!([]),
        )
        .await?;
        let pin = p.revision("work:a").await?;
        p.save(
            "work:b",
            Some("criterion:a"),
            contract("B", "Use the prerequisite"),
            json!([{"target":"work:a","relation":"depends_on","revision":pin,"blocking":true}]),
        )
        .await?;
        // Control: B pins the current revision of A.
        let b = p.object_both("work:b").await?;
        ensure!(b["review_required"] == false, "B before the change: {b}");
        p.claim("agent", "criterion:a").await?;
        let save = json!({"op":"save_object","kind":"work","id":"a","revision":pin,"parent":"criterion:a",
            "data":contract("A","Deliver the changed prerequisite"),"links":[],"reason":"Change the contract"});
        p.send(mcp, &save).await?.map_err(anyhow::Error::msg)?;
        let a = p.object_both("work:a").await?;
        ensure!(
            a["current_revision"] == pin + 1 && a["process"]["status"] == "open",
            "A after the change: {a}"
        );
        let b = p.object_both("work:b").await?;
        ensure!(
            b["review_required"] == true && b["document"]["links"][0]["revision"] == pin,
            "B after the change: {b}"
        );
        let waiting = p.tasks_both("waiting").await?;
        let entry = waiting
            .iter()
            .find(|t| t["address"] == "work:b")
            .ok_or_else(|| anyhow::anyhow!("B is missing in view=waiting: {waiting:?}"))?;
        ensure!(
            entry["claimable"] == false
                && entry["waiting_reason"]
                    == "Task has an unmet dependency or a changed dependency revision: work:a",
            "B in view=waiting: {entry}"
        );
        let available = p.tasks_both("available").await?;
        ensure!(
            !task_addresses(&available).contains(&"work:b"),
            "B must not be available: {available:?}"
        );
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}
