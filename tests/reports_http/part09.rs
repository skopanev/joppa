// Part 9 of tests/reports_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_batch_with_a_stale_test_list_save_changes_nothing() -> Result<()> {
    // checks-listed-first-ac4: a valid command and a stale list save in one batch.
    let w = world().await?;
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let workspace = w.base.rsplit('/').next().unwrap();
    let objects = || async {
        w.http
            .get(format!("{}/objects?parent=*&limit=100", w.base))
            .bearer_auth(&w.token)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await
    };
    let objects_before = objects().await?;
    let status_before = w.status().await?;
    let count_before = journal_count(&pool, workspace).await?;
    ensure!(status_before["tests_revision"] == 2, "{status_before}");
    let reply = planning_batch(&w, json!([
        {"command":{"op":"save_object","kind":"note","id":"batch-note","revision":0,"parent":null,"data":{"title":"batch-note","owner":"owner","body":"Signal"},"links":[],"reason":"Save"}},
        {"command":{"op":"set_ac_tests","target":"ac:key-rights","tests":[A],"revision":0,"reason":"Stale list"}}
    ])).await?;
    ensure!(
        reply["result"]["isError"] == true
            && reply["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|error| error.ends_with(STALE_LIST)),
        "{reply}"
    );
    let objects_after = objects().await?;
    ensure!(
        objects_after == objects_before,
        "{objects_before} / {objects_after}"
    );
    ensure!(
        !objects_after.to_string().contains("batch-note"),
        "{objects_after}"
    );
    let status_after = w.status().await?;
    ensure!(
        status_after == status_before,
        "{status_before} / {status_after}"
    );
    ensure!(journal_count(&pool, workspace).await? == count_before);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_journal_of_test_list_saves_without_revision_replays_to_the_same_lists() -> Result<()> {
    // checks-listed-first-ac4: the journal holds list saves with no revision field.
    let w = world().await?;
    w.list(&[A]).await?;
    w.list(&[B, A]).await?;
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let workspace = w.base.rsplit('/').next().unwrap();
    let conn = pool.get().await?;
    let saves: i64 = conn
        .query_one(
            "SELECT count(*) FROM joppa.events WHERE workspace=$1 AND command->>'op'='set_ac_tests' AND NOT command ? 'revision'",
            &[&workspace],
        )
        .await?
        .get(0);
    ensure!(saves == 3, "{saves} list saves without revision");
    let before = w.status().await?;
    ensure!(
        listed(&before) == [B, A] && before["tests_revision"] == 4,
        "{before}"
    );
    // Remove the stored lists and the board, then rebuild the workspace from its journal.
    for statement in [
        "DELETE FROM joppa.state_entries WHERE workspace=$1 AND section='ac_tests'",
        "DELETE FROM joppa.board_cache WHERE workspace=$1",
    ] {
        conn.execute(statement, &[&workspace]).await?;
    }
    store::backfill_board(&pool).await?;
    let after = w.status().await?;
    ensure!(
        listed(&after) == listed(&before) && after["tests_revision"] == before["tests_revision"],
        "{before} / {after}"
    );
    Ok(())
}

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
        s["tests"][0]["status"] == "fail" && s["tests"][0]["commit"] == "c1c1c1c",
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
        s["tests"][0]["status"] == "pass"
            && s["tests"][0]["commit"] == "c2c2c2c"
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
    ensure!(
        s["tests_draft"] == false && s["tests"].as_array().map(Vec::len) == Some(1),
        "{s}"
    );
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
            && s["status"] == "grey",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_todo_listed_test_keeps_the_criterion_grey() -> Result<()> {
    // missing-is-not-pass-ac3: X passes on C1; the later record for C2 has X as todo.
    let w = world().await?;
    let (code, reply) = post(
        &w,
        &w.owner,
        "/commands",
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"confirm","req":"requirement:key","revision":2}}),
    )
    .await?;
    ensure!(code == 200, "{code} {reply}");
    w.list(&[A]).await?;
    let s = w.status().await?;
    ensure!(
        s["tests_draft"] == false && s["tests"].as_array().map(Vec::len) == Some(1),
        "{s}"
    );
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
        s["tests"][0]["address"] == A && s["tests"][0]["status"] == "todo" && s["status"] == "grey",
        "{s}"
    );
    Ok(())
}
