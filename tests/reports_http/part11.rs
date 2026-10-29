// Part 11 of tests/reports_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn mcp_status_read_returns_100_criteria_in_request_order() -> Result<()> {
    // old-results-history-ac4: agent bot calls joppa_tests for A1 to A100 in one request.
    let (w, bot, addresses) = many_criteria().await?;
    let workspace = w.base.rsplit('/').next().unwrap();
    let first = addresses[..100].to_vec();
    let reply = mcp_read(
        &w,
        &bot,
        "joppa_tests",
        json!({"workspace":workspace,"ac":first.join(",")}),
    )
    .await?;
    ensure!(
        reply["items"].as_array().map(Vec::len) == Some(100),
        "item count: {reply}"
    );
    ensure!(item_addresses(&reply) == first, "{reply}");
    // 101 criteria in one call are refused.
    let reply = mcp(
        &w,
        &bot,
        "joppa_tests",
        json!({"workspace":workspace,"ac":addresses.join(",")}),
    )
    .await?;
    ensure!(
        reply["result"]["content"][0]["text"]
            .as_str()
            .is_some_and(|e| e.contains("at most 100 criteria in one call")),
        "{reply}"
    );

    // Control: an unknown address at position 100 returns its error and no items.
    let mut listed = addresses[..99].to_vec();
    listed.push("ac:missing".into());
    let reply = mcp(
        &w,
        &bot,
        "joppa_tests",
        json!({"workspace":workspace,"ac":listed.join(",")}),
    )
    .await?;
    ensure!(
        reply["result"]["isError"] == true
            && reply["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|e| e.contains("unknown criterion ac:missing")),
        "{reply}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn commits_behind_counts_newer_default_branch_commits() -> Result<()> {
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
        s["commit"] == "c1c1c1c" && s["commits_behind"] == 5 && s["status"] == "green",
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

async fn task(w: &World) -> Result<Value> {
    Ok(w.http
        .get(format!("{}/tests?task=task:t1", w.base))
        .bearer_auth(&w.token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_green_criterion_does_not_mark_an_open_task_done() -> Result<()> {
    // all-bound-checks-pass-ac1: an agent bot reads open task T under green criterion A.
    let w = world().await?;
    ensure!(
        task(&w).await?["done"] == false,
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
    let workspace = w.base.rsplit('/').next().unwrap();
    let t = mcp_read(
        &w,
        bot,
        "joppa_tests",
        json!({"workspace":workspace,"task":"task:t1"}),
    )
    .await?;
    // A green criterion does not complete the Task and gives it no commit.
    ensure!(
        t["done"] == false && t["commit"].is_null() && t.get("verification").is_none(),
        "{t}"
    );
    let object = mcp_read(
        &w,
        bot,
        "joppa_objects",
        json!({"workspace":workspace,"address":"task:t1"}),
    )
    .await?;
    ensure!(object["process"]["status"] == "open", "{object}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn only_complete_task_marks_a_task_done() -> Result<()> {
    let w = world().await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let workspace = w.base.rsplit('/').next().unwrap();
    for (token, command) in [
        (
            &w.owner,
            json!({"op":"save_object","kind":"task","id":"finished","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the key rights","owner":"owner","contract":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Check completion evidence"}),
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
    let read = |address: &'static str| {
        mcp_read(
            &w,
            bot,
            "joppa_tests",
            json!({"workspace":workspace,"task":address}),
        )
    };
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
            finished["done"] == true && finished["commit"] == "c0ffee0",
            "{finished}"
        );
        let open = read("task:t1").await?;
        ensure!(open["done"] == false && open["commit"].is_null(), "{open}");
    }
    Ok(())
}
