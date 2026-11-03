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

/// The process of the open Task t1 in the HTTP object read.
async fn task(w: &World) -> Result<Value> {
    task_process(w, &w.token, "task:t1", false).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_green_criterion_does_not_mark_an_open_task_done() -> Result<()> {
    // all-bound-checks-pass-ac1: an agent bot reads open task T under green criterion A.
    let w = world().await?;
    ensure!(
        task(&w).await?["status"] == "open",
        "not done before any report"
    );
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let t = task_process(&w, bot, "task:t1", true).await?;
    // A green criterion does not complete the Task and gives it no commit.
    ensure!(
        t["status"] == "open" && t["commit"].is_null() && t.get("verification").is_none(),
        "{t}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn only_complete_task_marks_a_task_done() -> Result<()> {
    let w = world().await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    for (token, command) in [
        (
            &w.owner,
            json!({"op":"save_object","kind":"task","id":"finished","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the key rights","owner":"owner","body":"Implement the key rights and record the result","projects":[]},"reason":"Check completion evidence"}),
        ),
        (
            &w.token,
            json!({"op":"assess","req":"requirement:key","revision":2,"ac":"ac:key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}}),
        ),
        (
            &w.token,
            json!({"op":"claim_task","id":"task:finished","revision":1}),
        ),
        (
            &w.token,
            json!({"op":"complete_task","commit":"c0ffee0","id":"task:finished","revision":2,"evidence":"Implemented the contract"}),
        ),
    ] {
        let (code, reply) = post(
            &w,
            token,
            "/commands",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
        )
        .await?;
        ensure!(code == 200, "{command}: {code} {reply}");
    }
    let read = |address: &'static str| task_process(&w, bot, address, true);
    for (commit, verification, cases) in [
        (None, "unverified", vec![]),
        (
            Some("c1c1c1c"),
            "failed",
            vec![("opens", "fail"), ("refuses", "pass")],
        ),
        (
            Some("c2c2c2c"),
            "passed",
            vec![("opens", "pass"), ("refuses", "pass")],
        ),
    ] {
        if let Some(commit) = commit {
            w.record(commit, outcomes(&cases)).await?;
        }
        // Only complete_task sets done and commit; records change neither.
        let _ = verification;
        let finished = read("task:finished").await?;
        ensure!(
            finished["status"] == "done" && finished["commit"] == "c0ffee0",
            "{finished}"
        );
        let open = read("task:t1").await?;
        ensure!(
            open["status"] == "open" && open["commit"].is_null(),
            "{open}"
        );
    }
    Ok(())
}
