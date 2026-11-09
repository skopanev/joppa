// Part 9 of tests/reports_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_red_test_stays_red_when_a_newer_record_omits_it() -> Result<()> {
    // defect-stays-red-ac1: opens fails on C1; a record for C2 has only refuses.
    let w = world().await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    w.record("c2c2c2c", outcomes(&[("refuses", "pass")]))
        .await?;
    let s = w.status().await?;
    ensure!(
        test_of(&s, A)["status"] == "fail" && test_of(&s, A)["commit"] == "c1c1c1c",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_red_test_clears_when_a_newer_commit_passes_it() -> Result<()> {
    // defect-stays-red-ac2
    let w = world().await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    w.record(
        "c2c2c2c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let s = w.status().await?;
    ensure!(
        test_of(&s, A)["status"] == "pass"
            && test_of(&s, A)["commit"] == "c2c2c2c"
            && s["status"] == "green",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_skipped_listed_test_keeps_the_criterion_grey() -> Result<()> {
    // missing-is-not-pass-ac1: X passes on C1; the later record for C2 has X skipped.
    let w = world().await?;
    w.list(&[A]).await?;
    let s = w.status().await?;
    ensure!(s["tests"].as_array().map(Vec::len) == Some(1), "{s}");
    w.record("c1c1c1c", outcomes(&[("opens", "pass")])).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == A
            && s["tests"][0]["status"] == "pass"
            && s["status"] == "green",
        "{s}"
    );
    w.record("c2c2c2c", outcomes(&[("opens", "skip")])).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == A
            && s["tests"][0]["status"] == "skipped"
            && s["status"] == "blue",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_todo_listed_test_keeps_the_criterion_grey() -> Result<()> {
    // missing-is-not-pass-ac3: X passes on C1; the later record for C2 has X as todo.
    let w = world().await?;
    w.list(&[A]).await?;
    let s = w.status().await?;
    ensure!(s["tests"].as_array().map(Vec::len) == Some(1), "{s}");
    w.record("c1c1c1c", outcomes(&[("opens", "pass")])).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == A
            && s["tests"][0]["status"] == "pass"
            && s["status"] == "green",
        "{s}"
    );
    w.record("c2c2c2c", outcomes(&[("opens", "todo")])).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == A && s["tests"][0]["status"] == "todo" && s["status"] == "blue",
        "{s}"
    );
    Ok(())
}
