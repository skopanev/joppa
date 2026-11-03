// Part 11 of tests/reports_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_criterion_shows_the_oldest_commit_of_its_tests_and_no_commit_count() -> Result<()> {
    // time-not-stale-ac1: A is decided on C; five newer commits ran other tests only.
    let w = world().await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    for order in 2..=6 {
        let other = json!([{"test":"tests/other.test.ts > other","status":"pass"}]);
        w.record(
            &format!("{order}{order}{order}{order}{order}{order}{order}"),
            other,
        )
        .await?;
    }
    let s = w.status().await?;
    ensure!(
        s["commit"] == "c1c1c1c" && s.get("commits_behind").is_none() && s["status"] == "green",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_test_tag_in_code_links_nothing() -> Result<()> {
    // binding-pins-contract-ac1: "[ac:key-rights] other" fails in the same record.
    let w = world().await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(w.status().await?["status"] == "green", "green before");
    w.record(
        "c2c2c2c",
        outcomes(&[
            ("opens", "pass"),
            ("refuses", "pass"),
            ("[ac:key-rights] other", "fail"),
        ]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "a tag in a test name links nothing"
    );
    Ok(())
}
