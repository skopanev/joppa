// Part 31 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_holding_a_task_claims_a_task_in_another_workspace() -> Result<()> {
    // one-task-per-agent-ac2
    let w = world().await?;
    let assess = json!({"op":"assess","req":"requirement:key","revision":2,"ac":"ac:key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}});
    w.command_as(&w.bot, assess.clone()).await?;
    let t1 = ready_task(&w, "t1", "key-rights").await?;
    w.command_as(&w.bot, json!({"op":"claim_task","id":t1,"revision":1}))
        .await?;
    let w2 = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    seed(
        &w.pool,
        &w2,
        vec![
            ("bot", assess),
            ("owner", ready_task_command("t3", "key-rights")),
        ],
    )
    .await?;
    let t3 = "task:t3";

    let reply = w
        .http
        .post(format!("{}/api/workspaces/{w2}/commands", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"claim_task","id":t3,"revision":1}}))
        .send()
        .await?;
    let status = reply.status();
    let body = reply.text().await?;
    let read_t3 = w
        .http
        .get(format!(
            "{}/api/workspaces/{w2}/objects?address={t3}",
            w.root
        ))
        .bearer_auth(&w.owner)
        .send()
        .await?;
    ensure!(read_t3.status().is_success(), "{}", read_t3.text().await?);
    let t3: Value = read_t3.json().await?;
    let t1 = w.get(&format!("address={t1}")).await?;
    let states = format!("reply {status} {body}; T3 {t3}; T1 {t1}");
    ensure!(status == reqwest::StatusCode::OK, "{states}");
    ensure!(
        t3["process"]["assignee"] == "bot" && t3["process"]["status"] == "in_progress",
        "{states}"
    );
    ensure!(t1["process"]["assignee"] == "bot", "{states}");
    Ok(())
}

/// `token` sends `command` over HTTP and then MCP. Each transport must refuse with `expected`
/// and keep the Task revision, process.blocked, the journal position and all saved rows.
async fn refuse_task_action(
    w: &World,
    case: &str,
    token: &str,
    command: Value,
    expected: &str,
) -> Result<()> {
    let address = command["id"].as_str().unwrap();
    for transport in ["HTTP", "MCP"] {
        let task = w.get(&format!("address={address}")).await?;
        let before = saved_rows(w).await?;
        let request_id = uuid::Uuid::new_v4().to_string();
        let (refused, error, reply) = if transport == "MCP" {
            let result = mcp_call(
                w,
                token,
                "joppa_command",
                json!({"workspace":w.ws,"request_id":request_id,"command":command}),
            )
            .await?;
            (
                result["isError"] == true,
                result["content"][0]["text"].clone(),
                result,
            )
        } else {
            let response = w
                .http
                .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
                .bearer_auth(token)
                .json(&json!({"request_id":request_id,"command":command}))
                .send()
                .await?;
            let refused = response.status().is_client_error();
            let reply: Value = response.json().await?;
            (refused, reply["error"].clone(), reply)
        };
        ensure!(
            refused && error == expected,
            "{case} over {transport}: expected {expected:?}, got {reply}"
        );
        let after = w.get(&format!("address={address}")).await?;
        let rows = saved_rows(w).await?;
        ensure!(
            after["current_revision"] == task["current_revision"]
                && after["process"]["blocked"] == task["process"]["blocked"]
                && rows["position"] == before["position"]
                && rows == before,
            "{case} over {transport} changed saved state: {reply}"
        );
    }
    Ok(())
}

// failed-task-blocks-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn block_and_blocked_task_actions_refuse_exact_cases_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    // L holds T, M is another member, Z is an absent agent that holds D, and S is the owner.
    let mut tokens = vec![];
    for agent in ["lead", "mate", "zed"] {
        let created = store::create_agent_token(&w.pool, &w.ws, "owner", agent, None).await?;
        tokens.push(created["token"].as_str().unwrap().to_owned());
    }
    let (l, m, z, s) = (&tokens[0], &tokens[1], &tokens[2], &w.owner);
    w.command_as(l, json!({"op":"assess","req":"requirement:key","revision":2,"ac":"ac:key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    for id in ["t", "u", "d"] {
        w.command(json!({"op":"save_object","kind":"task","id":id,"revision":0,"parent":"ac:key-rights","data":{"title":"Complete the defined work","owner":"owner","body":"Implement the key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Create work"})).await?;
    }
    w.command_as(l, json!({"op":"claim_task","id":"task:t","revision":1}))
        .await?;
    w.command_as(
        l,
        json!({"op":"release_task","id":"task:t","revision":2,"reason":"Return the work"}),
    )
    .await?;
    w.command_as(l, json!({"op":"claim_task","id":"task:t","revision":3}))
        .await?;
    w.command_as(z, json!({"op":"claim_task","id":"task:d","revision":1}))
        .await?;
    let t = w.get("address=task:t").await?;
    ensure!(
        t["current_revision"] == 4 && t["process"]["assignee"] == "lead",
        "{t}"
    );
    let block = |id: &str, revision: u64, reason: &str, evidence: &str| json!({"op":"block_task","id":id,"revision":revision,"reason":reason,"evidence":evidence});
    let (reason, evidence) = ("Verification failed", "test x failed on abc123");

    refuse_task_action(
        &w,
        "M blocks T",
        m,
        block("task:t", 4, reason, evidence),
        "Task holder required",
    )
    .await?;
    refuse_task_action(
        &w,
        "L blocks T at revision 3",
        l,
        block("task:t", 3, reason, evidence),
        "stale Task revision; read the Task again",
    )
    .await?;
    refuse_task_action(
        &w,
        "L blocks T with a blank reason",
        l,
        block("task:t", 4, " ", evidence),
        "text must not be empty",
    )
    .await?;
    refuse_task_action(
        &w,
        "L blocks T with blank evidence",
        l,
        block("task:t", 4, reason, " "),
        "text must not be empty",
    )
    .await?;
    refuse_task_action(
        &w,
        "L blocks U",
        l,
        block("task:u", 1, reason, evidence),
        "Task is not claimed unfinished work",
    )
    .await?;

    w.command_as(l, block("task:t", 4, reason, evidence))
        .await?;
    let t = w.get("address=task:t").await?;
    ensure!(
        t["current_revision"] == 5
            && t["process"]["blocked"]["actor"] == "lead"
            && t["process"]["assignee"].is_null(),
        "{t}"
    );
    refuse_task_action(
        &w,
        "L claims blocked T",
        l,
        json!({"op":"claim_task","id":"task:t","revision":5}),
        "Task is blocked for planning",
    )
    .await?;
    refuse_task_action(&w, "L completes blocked T", l, json!({"op":"complete_task","commit":"c0ffee0","id":"task:t","revision":5,"evidence":"Implemented and checked"}), "Task is not claimed unfinished work").await?;
    refuse_task_action(
        &w,
        "S releases blocked T",
        s,
        json!({"op":"release_task","id":"task:t","revision":5,"reason":"Return the work"}),
        "Task is blocked for planning",
    )
    .await?;

    w.command_as(
        s,
        block("task:d", 2, "Agent Z is absent", "No heartbeat from zed"),
    )
    .await?;
    let d = w.get("address=task:d").await?;
    ensure!(
        d["process"]["blocked"]["actor"] == "owner" && d["process"]["assignee"].is_null(),
        "{d}"
    );
    store::check_journal(&w.pool).await?;
    Ok(())
}
