// Part 12 of tests/reports_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_joppa_tests_tool_states_that_only_complete_task_sets_done() -> Result<()> {
    let w = world().await?;
    let root = w.base.split("/api/").next().unwrap();
    let reply: Value = w
        .http
        .post(format!("{root}/mcp"))
        .bearer_auth(&w.token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await?
        .json()
        .await?;
    let description = reply["result"]["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|t| t["name"] == "joppa_tests"))
        .and_then(|t| t["description"].as_str())
        .ok_or_else(|| anyhow::anyhow!("no joppa_tests: {reply}"))?;
    ensure!(
        description.contains("Only complete_task sets done and commit")
            && !description.contains("verification follows the criterion status")
            && !description.contains("while its criterion is green"),
        "{description}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_is_no_longer_done_when_its_criterion_turns_red() -> Result<()> {
    // all-bound-checks-pass-ac2: a newer commit makes A red.
    let w = world().await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(task(&w).await?["done"] == false, "not completed on c1");
    w.record(
        "c2c2c2c",
        outcomes(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    let t = task(&w).await?;
    ensure!(
        t["done"] == false && t["commit"].is_null() && t.get("verification").is_none(),
        "{t}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_completed_task_stays_done_when_its_criterion_turns_red() -> Result<()> {
    // all-bound-checks-pass-ac2: complete_task finished T; a newer commit makes A red.
    let w = world().await?;
    for (token, command) in [
        (
            &w.owner,
            json!({"op":"save_object","kind":"task","id":"finished","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the key rights","owner":"owner","body":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Check completion evidence"}),
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
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let workspace = w.base.rsplit('/').next().unwrap();
    for (commit, verification, status, cases) in [
        (
            "c1c1c1c",
            "passed",
            "green",
            [("opens", "pass"), ("refuses", "pass")],
        ),
        (
            "c2c2c2c",
            "failed",
            "red",
            [("opens", "fail"), ("refuses", "pass")],
        ),
    ] {
        w.record(commit, outcomes(&cases)).await?;
        let response = w
            .http
            .get(format!("{}/tests?task=task:finished", w.base))
            .bearer_auth(bot)
            .send()
            .await?;
        let code = response.status().as_u16();
        let http: Value = response.json().await?;
        ensure!(code == 200, "HTTP {commit}: {code} {http}");
        let mcp = mcp_read(
            &w,
            bot,
            "joppa_tests",
            json!({"workspace":workspace,"task":"task:finished"}),
        )
        .await?;
        // The Task keeps its own state and commit; the criterion status does not change it.
        let _ = (verification, status);
        for (via, t) in [("HTTP", &http), ("MCP", &mcp)] {
            ensure!(
                t["done"] == true && t["commit"] == "c0ffee0" && t.get("verification").is_none(),
                "{via} {commit}: {t}"
            );
        }
    }
    let object = mcp_read(
        &w,
        bot,
        "joppa_objects",
        json!({"workspace":workspace,"address":"task:finished"}),
    )
    .await?;
    ensure!(object["process"]["status"] == "done", "{object}");
    Ok(())
}

async fn post(w: &World, token: &str, path: &str, body: Value) -> Result<(u16, Value)> {
    let reply = w
        .http
        .post(format!("{}{path}", w.base))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;
    Ok((
        reply.status().as_u16(),
        reply.json().await.unwrap_or(Value::Null),
    ))
}

async fn save_note(w: &World, token: &str, id: &str) -> Result<(u16, Value)> {
    let command = json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"owner","body":"Signal"},"links":[],"reason":"Save"});
    post(
        w,
        token,
        "/commands",
        json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}),
    )
    .await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_token_writes_as_the_agent_and_stops_when_revoked() -> Result<()> {
    // agent-own-identity-ac1, write-records-agent-ac1, removal-at-once-ac2
    let w = world().await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(
        code == 200
            && created["token"]
                .as_str()
                .is_some_and(|t| t.starts_with("jpt_")),
        "{code} {created}"
    );
    let bot = created["token"].as_str().unwrap().to_owned();
    let (code, saved) = save_note(&w, &bot, "from-bot").await?;
    ensure!(code == 200, "{code} {saved}");
    let read: Value = w
        .http
        .get(format!("{}/objects?address=note:from-bot", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    ensure!(
        read["document"]["actor"] == "bot",
        "the write records the agent: {read}"
    );
    let id = created["id"].as_i64().unwrap();
    let (code, _) = post(
        &w,
        &w.owner,
        &format!("/agent-tokens/{id}/revoke"),
        json!({}),
    )
    .await?;
    ensure!(code == 200, "revoke");
    let (code, reply) = save_note(&w, &bot, "after-revoke").await?;
    ensure!(
        code == 401 && reply["error"] == "token revoked",
        "{code} {reply}"
    );
    Ok(())
}
