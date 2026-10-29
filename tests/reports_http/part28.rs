// Part 28 of tests/reports_http.rs, included there, so the test names stay unchanged.

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f5_an_identical_list_save_makes_a_grey_and_unassessed() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    let r = p.read("spec:r").await?;
    ensure!(
        p.assessment("criterion:a").await?["state"] == "ready",
        "A must start ready"
    );
    p.resave_a().await?;
    p.reset(&before).await?;
    let after = p.read("spec:r").await?;
    ensure!(
        after["current_revision"] == r["current_revision"]
            && after["process"]["revision"] == r["process"]["revision"],
        "R has a new revision: {after}"
    );
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f6_an_archived_red_sibling_makes_a_and_r_grey() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    let r = p.get("/objects?address=spec:r".into()).await?;
    ensure!(r["verification_summary"]["passed"] == 1, "{r}");
    let revision = p.revision("criterion:b").await?;
    p.post_command(&w.owner, json!({"op":"archive_object","target":"criterion:b","revision":revision,"reason":"Drop the red criterion"})).await?;
    p.reset(&before).await?;
    let r = p
        .mcp_read("joppa_objects", json!({"address":"spec:r"}))
        .await?;
    ensure!(
        r["verification_summary"] == json!({"total":1,"passed":0,"failed":0}),
        "MCP joppa_objects R: {r}"
    );
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f6_control_an_archived_inbox_signal_keeps_a_green() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    p.signal().await?;
    let (before, assessment) = (
        p.ac_tests("criterion:a").await?,
        p.assessment("criterion:a").await?,
    );
    p.post_command(
        &w.owner,
        json!({"op":"archive_object","target":"signal:s","revision":1,"reason":"Drop the signal"}),
    )
    .await?;
    p.kept(&before, &assessment).await
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f7_a_pm_marks_grey_a_ready_and_a_stays_grey() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    p.resave_a().await?;
    p.reset(&before).await?;
    let revision = p.read("spec:r").await?["process"]["revision"].clone();
    p.write("owner", json!({"op":"assess","req":"spec:r","revision":revision,"ac":"criterion:a","assessment":{"ready":true,"reason":"Known scope"}})).await?;
    let readiness = p.assessment("criterion:a").await?;
    let s = p.ac_tests("criterion:a").await?;
    ensure!(
        readiness["state"] == "ready" && s["status"] == "grey",
        "{readiness} {s}"
    );
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f8_only_a_record_after_the_change_makes_a_green() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    let b = (
        p.ac_tests("criterion:b").await?,
        p.assessment("criterion:b").await?,
    );
    p.resave_a().await?;
    // Control: the record from before the change does not count.
    p.reset(&before).await?;
    // Control: a record after the change that misses a listed test stays grey.
    p.record("c2c2c2c", outcomes(&[("opens", "pass")])).await?;
    let s = p.ac_tests("criterion:a").await?;
    ensure!(s["status"] == "grey", "after records: {s}");
    p.record("c4c4c4c", frozen_run()).await?;
    let s = p.ac_tests("criterion:a").await?;
    ensure!(s["status"] == "green", "after the record: {s}");
    let mcp = p
        .mcp_read("joppa_tests", json!({"ac":"criterion:a"}))
        .await?;
    ensure!(mcp["status"] == "green", "MCP joppa_tests: {mcp}");
    let after = (
        p.ac_tests("criterion:b").await?,
        p.assessment("criterion:b").await?,
    );
    ensure!(
        after.0["status"] == b.0["status"] && after.1 == b.1,
        "sibling B changed: {after:?}"
    );
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f9_finish_planning_under_green_a_keeps_a_green() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, true).await?;
    let (before, assessment) = (
        p.ac_tests("criterion:a").await?,
        p.assessment("criterion:a").await?,
    );
    p.post_command(
        &w.token,
        json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"}),
    )
    .await?;
    let a = p.read("criterion:a").await?;
    ensure!(a["process"]["planning_claim"].is_null(), "{a}");
    p.kept(&before, &assessment).await
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f10_an_inbox_signal_under_r_keeps_a_green() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let (before, assessment) = (
        p.ac_tests("criterion:a").await?,
        p.assessment("criterion:a").await?,
    );
    p.signal().await?;
    p.kept(&before, &assessment).await
}

// signal-becomes-spec-ac3
// Member alex archives one signal under R over HTTP and one over MCP. A and R keep
// their status, readiness and verification summary. Control: a new title of R makes A grey.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_archive_of_a_signal_under_r_keeps_a_green_and_ready() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    p.write("owner", json!({"op":"member","id":"alex","role":"human"}))
        .await?;
    for id in ["http-n", "mcp-n"] {
        p.post_command(&w.owner, json!({"op":"save_object","kind":"signal","id":id,"revision":0,"parent":null,"data":{"title":"Provider change","owner":"owner","body":"New permission rules"},"links":[{"target":"spec:r","relation":"related"}],"reason":"Record the signal"})).await?;
    }
    let state = async || -> Result<Value> {
        let a = p
            .mcp_read("joppa_objects", json!({"address":"criterion:a"}))
            .await?;
        let r = p
            .mcp_read("joppa_objects", json!({"address":"spec:r"}))
            .await?;
        Ok(json!({
            "status": p.ac_tests("criterion:a").await?["status"],
            "assessment": a["process"]["assessment"],
            "summary": r["verification_summary"],
        }))
    };
    let before = state().await?;
    ensure!(
        before["status"] == "green" && before["assessment"]["state"] == "ready",
        "A must start green and ready: {before}"
    );
    for (mcp, signal) in [(false, "signal:http-n"), (true, "signal:mcp-n")] {
        let archive =
            json!({"op":"archive_object","target":signal,"revision":1,"reason":"Needs no work"});
        let reply = p.send_as(&w.alex, mcp, &archive).await?;
        ensure!(reply.is_ok(), "mcp={mcp}: {reply:?}");
        let after = state().await?;
        ensure!(after == before, "mcp={mcp}: {before} became {after}");
        let n = p
            .mcp_read("joppa_objects", json!({"address":signal}))
            .await?;
        ensure!(n["document"]["present"] == false, "mcp={mcp}: {n}");
    }
    // Control: the fixture detects a reset.
    let tests = p.ac_tests("criterion:a").await?;
    p.save_r("area:g", "R renamed", json!([])).await?;
    p.reset(&tests).await
}
