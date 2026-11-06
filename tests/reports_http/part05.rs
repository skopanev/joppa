// Part 5 of tests/reports_http.rs, included there, so the test names stay unchanged.

// status-current-only-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_listed_test_without_a_result_is_not_run_and_grey() -> Result<()> {
    // A lists opens (X) and refuses (Y); the only record of C with X passing.
    let w = world().await?;
    w.record("c1c1c1c", outcomes(&[("opens", "pass")])).await?;
    let s = w.status().await?;
    // The agent reads the same grey tone.
    ensure!(tone_at(&w.http, &w.base, &w.token, "ac:key-rights").await? == "grey");
    let test = |address: &str| {
        s["tests"]
            .as_array()
            .and_then(|tests| tests.iter().find(|t| t["address"] == address))
    };
    ensure!(
        test(A).is_some_and(|x| x["status"] == "pass" && x["commit"] == "c1c1c1c")
            && test(B).is_some_and(|y| y["status"] == "not_run")
            && s["status"] == "grey"
            && s["reason"] == "not all tests passed",
        "{s}"
    );
    Ok(())
}
