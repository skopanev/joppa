// Part 6 of tests/reports_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_criterion_save_cannot_set_a_test_result() -> Result<()> {
    // no-manual-pass-ac4: bot saves A with a verification or a passing list; X stays not_run.
    let w = world().await?;
    w.list(&[A]).await?;
    let x_status = |s: &Value| {
        s["tests"]
            .as_array()
            .and_then(|tests| tests.iter().find(|t| t["address"] == A))
            .map(|x| x["status"].clone())
    };
    let s = w.status().await?;
    ensure!(x_status(&s) == Some(json!("not_run")), "{s}");
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(
        code == 200 && created["token"].as_str().is_some_and(|t| !t.is_empty()),
        "{code} {created}"
    );
    let bot = created["token"].as_str().unwrap().to_owned();
    let save = |data: Value, criterion: Value| {
        let command = json!({"op":"save_object","kind":"ac","id":"key-rights","revision":criterion["current_revision"],"parent":criterion["document"]["parent"],"data":data,"links":criterion["document"]["links"],"reason":"Save the criterion"});
        post(
            &w,
            &bot,
            "/commands",
            json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}),
        )
    };
    // Control: a valid data change saves.
    let criterion = planning_criterion(&w).await?;
    let mut data = criterion["document"]["data"].clone();
    data["body"] =
        json!("Given: a key.\nWhen: bot reads.\nThen: bot can read.\nEvidence: the test report.");
    let (code, reply) = save(data, criterion).await?;
    ensure!(code == 200, "control save: {code} {reply}");
    let before = w.status().await?;
    // Fixture 1: data.verification="passed" is refused.
    let criterion = planning_criterion(&w).await?;
    let mut data = criterion["document"]["data"].clone();
    data["verification"] = json!("passed");
    let (code, reply) = save(data, criterion).await?;
    ensure!(
        code != 200 && reply["error"] == "verification comes from test reports",
        "fixture 1: {code} {reply}"
    );
    let after = w.status().await?;
    ensure!(
        after == before && x_status(&after) == Some(json!("not_run")),
        "fixture 1: {before} {after}"
    );
    // Fixture 2: an extra field that lists X as passing may save or be refused.
    let before = w.status().await?;
    let criterion = planning_criterion(&w).await?;
    let mut data = criterion["document"]["data"].clone();
    data["test_results"] = json!([{"address": A, "status": "pass"}]);
    let (code, reply) = save(data, criterion).await?;
    let after = w.status().await?;
    // A save gives the document a revision, so only tests_revision may change.
    ensure!(
        after["status"] == before["status"]
            && after["tests"] == before["tests"]
            && x_status(&after) == Some(json!("not_run")),
        "fixture 2 ({code} {reply}): {before} {after}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_text_change_greys_the_criterion_until_a_newer_record() -> Result<()> {
    // check-list-frozen-ac1 and ac2: the text changes, and only a newer record counts.
    let w = world().await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "green before the text change"
    );
    w.revise_text(1, "Only read and verify until launch")
        .await?;
    let s = w.status().await?;
    // check-list-frozen-ac2: the report before the text change no longer counts.
    ensure!(
        s["status"] == "grey" && s["reason"] == "not all tests passed" && listed(&s) == [A, B],
        "{s}"
    );
    w.record(
        "c2c2c2c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "a new record after the change is green"
    );
    Ok(())
}

/// The test list fields that a report must not change.
fn saved_list(status: &Value) -> Value {
    json!([
        addresses(status),
        status["tests_revision"],
        status["tests_saved_by"]
    ])
}

/// Report and result rows of this workspace.
async fn report_rows(w: &World) -> Result<(i64, i64)> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let workspace = w.base.rsplit('/').next().unwrap();
    let row = pool.get().await?.query_one("SELECT (SELECT count(*) FROM joppa.test_reports WHERE workspace=$1),(SELECT count(*) FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1)", &[&workspace]).await?;
    Ok((row.get(0), row.get(1)))
}

/// A clean full C1 report passes every test of A, then the text of A changes: A is grey.
async fn changed_after_c1(w: &World) -> Result<(Value, Value)> {
    let first = w.record("c1c1c1c", passing()).await?;
    // A first record gives no replayed flag.
    ensure!(first.get("replayed").is_none(), "{first}");
    ensure!(
        w.status().await?["status"] == "green",
        "green before the text change"
    );
    w.revise_text(1, "Only read and verify until launch")
        .await?;
    let s = w.status().await?;
    ensure!(s["status"] == "grey", "before the record: {s}");
    Ok((first, s))
}

fn passing() -> Value {
    outcomes(&[("opens", "pass"), ("refuses", "pass")])
}

// check-list-frozen-ac4, fixture 1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_identical_c1_report_after_a_text_change_turns_the_criterion_green() -> Result<()> {
    let w = world().await?;
    let (first, before) = changed_after_c1(&w).await?;
    let rows = report_rows(&w).await?;
    let again = w.record("c1c1c1c", passing()).await?;
    ensure!(
        again["record"] == first["record"] && again["replayed"] == true,
        "{again}"
    );
    ensure!(
        report_rows(&w).await? == rows,
        "the repeat copied report rows or results"
    );
    let s = w.status().await?;
    ensure!(
        s["status"] == "green" && saved_list(&s) == saved_list(&before),
        "after the record: {s}"
    );
    let workspace = w.base.rsplit('/').next().unwrap();
    let mcp = mcp_read(
        &w,
        &w.token,
        "joppa_tests",
        json!({"workspace":workspace,"ac":"ac:key-rights"}),
    )
    .await?;
    ensure!(mcp["status"] == "green", "MCP joppa_tests: {mcp}");
    Ok(())
}

// check-list-frozen-ac4, fixture 2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_c2_report_after_a_text_change_turns_the_criterion_green() -> Result<()> {
    let w = world().await?;
    let (_, before) = changed_after_c1(&w).await?;
    w.record("c2c2c2c", passing()).await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "green" && saved_list(&s) == saved_list(&before),
        "after the record: {s}"
    );
    Ok(())
}

// check-list-frozen-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn no_report_or_only_partial_reports_after_a_text_change_keep_the_criterion_grey()
-> Result<()> {
    let w = world().await?;
    let (_, before) = changed_after_c1(&w).await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "grey" && saved_list(&s) == saved_list(&before),
        "without a new report: {s}"
    );
    // A record that covers only one listed test keeps the criterion grey.
    w.record("c2c2c2c", outcomes(&[("opens", "pass")])).await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "grey" && saved_list(&s) == saved_list(&before),
        "after records: {s}"
    );
    Ok(())
}

// check-list-frozen-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_text_change_without_a_stored_boundary_counts_a_newer_record() -> Result<()> {
    let w = world().await?;
    let (_, before) = changed_after_c1(&w).await?;
    // A criterion whose text changed before boundaries were stored has none. The test
    // list is a field of its document, so no draft waits for another save.
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let workspace = w.base.rsplit('/').next().unwrap();
    let removed = pool
        .get()
        .await?
        .execute(
            "DELETE FROM joppa.state_entries WHERE workspace=$1 AND section='criterion_boundaries'",
            &[&workspace],
        )
        .await?;
    ensure!(removed == 1, "the text change stored no boundary");
    w.record("c2c2c2c", passing()).await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "green" && saved_list(&s) == saved_list(&before),
        "a text change without a boundary: {s}"
    );
    Ok(())
}
