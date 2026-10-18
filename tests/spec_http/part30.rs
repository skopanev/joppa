// Part 30 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_holding_a_task_cannot_claim_a_task_of_another_req() -> Result<()> {
    let w = world().await?;
    w.command(json!({"op":"revise","req":"other","expected_revision":0,"capability":"restrict","title":"Other rights","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"other-rights","text":"Only write after review"}]})).await?;
    for (req, ac) in [("key", "key-rights"), ("other", "other-rights")] {
        w.command_as(&w.bot, json!({"op":"assess","req":req,"revision":1,"ac":ac,"assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    }
    let held = ready_task(&w, "a1", "key-rights").await?;
    let same = ready_task(&w, "a2", "key-rights").await?;
    let other = ready_task(&w, "b1", "other-rights").await?;
    w.command_as(&w.bot, json!({"op":"claim_task","id":held,"revision":1}))
        .await?;
    let queue = async |view: &str| {
        mcp_value(&w, "joppa_tasks", json!({"workspace":w.ws,"view":view})).await
    };
    let available = queue("available").await?;
    ensure!(
        available["tasks"].as_array().unwrap().len() == 1
            && available["tasks"][0]["address"] == same
            && available["tasks"][0]["claimable"] == true,
        "{available}"
    );
    let waiting = queue("waiting").await?;
    let reason = format!("agent already holds task {held}");
    ensure!(
        waiting["tasks"].as_array().unwrap().len() == 1
            && waiting["tasks"][0]["address"] == other
            && waiting["tasks"][0]["claimable"] == false
            && waiting["tasks"][0]["waiting_reason"] == reason,
        "{waiting}"
    );
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.bot)
        .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"claim_task","id":other,"revision":1}}))
        .send()
        .await?;
    ensure!(
        reply.status() == reqwest::StatusCode::CONFLICT,
        "{}",
        reply.status()
    );
    let reply: Value = reply.json().await?;
    ensure!(reply["error"] == reason, "{reply}");
    let refused = w.get(&format!("address={other}")).await?;
    ensure!(refused["process"]["assignee"].is_null(), "{refused}");
    let next = w
        .command_as(&w.bot, json!({"op":"claim_next_task"}))
        .await?;
    ensure!(next["command"]["id"] == same, "{next}");
    Ok(())
}

/// REQ key holds task:t1; REQ other holds task:t2 under other-rights and a second AC other-audit.
async fn tasks_under_two_requirements(w: &World) -> Result<()> {
    w.command(json!({"op":"revise","req":"other","expected_revision":0,"capability":"restrict","title":"Other rights","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"other-rights","text":"Only write after review"},{"id":"other-audit","text":"Record each write"}]})).await?;
    for (req, ac) in [
        ("key", "key-rights"),
        ("other", "other-rights"),
        ("other", "other-audit"),
    ] {
        w.command_as(&w.bot, json!({"op":"assess","req":req,"revision":1,"ac":ac,"assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    }
    ready_task(w, "t1", "key-rights").await?;
    ready_task(w, "t2", "other-rights").await?;
    Ok(())
}

/// A Task save with one blocking link to `target` at revision 1.
fn linked_task_command(id: &str, revision: u32, ac: &str, target: &str) -> Value {
    let mut command = ready_task_command(id, ac);
    command["revision"] = json!(revision);
    command["links"] =
        json!([{"target":target,"relation":"depends_on","revision":1,"blocking":true}]);
    command
}

// task-links-stay-in-req-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_link_to_a_task_of_another_requirement_is_refused() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    let error = "a Task can depend only on a Task under the same requirement";
    let t2 = w.get("address=task:t2").await?;
    ensure!(t2["document"]["links"] == json!([]), "{t2}");
    let saved = saved_rows(&w).await?;
    for command in [
        linked_task_command("t3", 0, "other-rights", "task:t1"),
        linked_task_command("t2", 1, "other-rights", "task:t1"),
    ] {
        let (ok, http) = send_as(&w, &w.bot, &command).await?;
        ensure!(!ok && http["error"] == error, "HTTP {command}: {http}");
        let mcp = mcp_call(
            &w,
            &w.bot,
            "joppa_command",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
        )
        .await?;
        ensure!(
            mcp["isError"] == true && mcp["content"][0]["text"] == error,
            "MCP {command}: {mcp}"
        );
    }
    ensure!(
        saved_rows(&w).await? == saved,
        "a refused Task link stored an object, revision or journal row"
    );
    let after = w.get("address=task:t2").await?;
    ensure!(
        after["document"]["links"] == json!([])
            && after["current_revision"] == t2["current_revision"],
        "{after}"
    );
    let t3 = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/objects?address=task:t3",
            w.root, w.ws
        ))
        .bearer_auth(&w.bot)
        .send()
        .await?;
    let status = t3.status();
    let t3: Value = t3.json().await?;
    ensure!(
        !status.is_success() && t3 == json!({"error":"unknown object"}),
        "{status}: {t3}"
    );
    Ok(())
}

// task-links-stay-in-req-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_link_across_criteria_of_one_requirement_is_saved() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    w.command_as(
        &w.bot,
        linked_task_command("t4", 0, "other-audit", "task:t2"),
    )
    .await?;
    let t4 = w.get_as(&w.bot, "address=task:t4").await?;
    ensure!(
        t4["document"]["links"]
            == json!([{"target":"task:t2","relation":"depends_on","revision":1,"blocking":true}]),
        "{t4}"
    );
    Ok(())
}

// task-links-stay-in-req-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_link_added_across_criteria_of_one_requirement_waits_for_its_dependency()
-> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    // Bot creates task:t4 so that bot, as its unclaimed creator, may add the link.
    w.command_as(&w.bot, ready_task_command("t4", "other-audit"))
        .await?;
    let t4 = "task:t4";
    let (ok, saved) = send_as(
        &w,
        &w.bot,
        &linked_task_command("t4", 1, "other-audit", "task:t2"),
    )
    .await?;
    ensure!(ok, "{saved}");
    let read = w.get_as(&w.bot, &format!("address={t4}")).await?;
    let links = &read["document"]["links"];
    ensure!(
        links[0]["target"] == "task:t2" && links[0]["blocking"] == true,
        "{read}"
    );
    let (ok, early) = send_as(
        &w,
        &w.bot,
        &json!({"op":"claim_task","id":t4,"revision":read["current_revision"]}),
    )
    .await?;
    ensure!(
        !ok && early["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("Task has an unmet dependency")),
        "{early}"
    );
    let waiting = w.get(&format!("address={t4}")).await?;
    ensure!(waiting["process"]["assignee"].is_null(), "{waiting}");
    let current = async |address: &str| -> Result<Value> {
        Ok(w.get(&format!("address={address}")).await?["current_revision"].clone())
    };
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"task:t2","revision":current("task:t2").await?}),
    )
    .await?;
    w.command_as(&w.bot, json!({"op":"complete_task","id":"task:t2","revision":current("task:t2").await?,"evidence":"Implemented the contract"})).await?;
    let (ok, claimed) = send_as(
        &w,
        &w.bot,
        &json!({"op":"claim_task","id":t4,"revision":current(t4).await?}),
    )
    .await?;
    ensure!(ok, "{claimed}");
    Ok(())
}

// task-links-stay-in-req-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_journal_with_a_cross_requirement_task_link_still_replays() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    // Seed the historical link straight into the journal; live saves now refuse it.
    let command = linked_task_command("t2", 1, "other-rights", "task:t1");
    let position = journal_position(&w).await? + 1;
    let conn = w.pool.get().await?;
    conn.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,'owner',$3,$4::text::jsonb)",
        &[&w.ws, &position, &uuid::Uuid::new_v4().to_string(), &command.to_string()],
    )
    .await?;
    conn.execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&w.ws])
        .await?;
    store::backfill_board(&w.pool).await?;
    let t2 = w.get("address=task:t2").await?;
    ensure!(
        t2["current_revision"] == 2
            && t2["document"]["links"]
                == json!([{"target":"task:t1","relation":"depends_on","revision":1,"blocking":true}]),
        "{t2}"
    );
    Ok(())
}
