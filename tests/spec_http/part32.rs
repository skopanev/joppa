// Part 32 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// One command over REST or MCP joppa_command: the reply, or the refusal text.
async fn send_over(
    w: &World,
    mcp: bool,
    token: &str,
    command: &Value,
) -> Result<Result<Value, String>> {
    if !mcp {
        let (ok, reply) = send_as(w, token, command).await?;
        return Ok(if ok {
            Ok(reply)
        } else {
            Err(reply["error"].as_str().unwrap_or_default().to_owned())
        });
    }
    let result = mcp_call(
        w,
        token,
        "joppa_command",
        json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
    )
    .await?;
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    Ok(if result["isError"] == true {
        Err(text.to_owned())
    } else {
        Ok(serde_json::from_str(text)?)
    })
}

/// The refusal text matches, and the saved rows with the journal stay the same.
async fn refused_without_change(
    w: &World,
    mcp: bool,
    token: &str,
    command: Value,
    error: &str,
) -> Result<()> {
    let before = saved_rows(w).await?;
    let reply = send_over(w, mcp, token, &command).await?;
    ensure!(
        reply.as_ref().err().map(String::as_str) == Some(error),
        "mcp={mcp} {command}: {reply:?}"
    );
    ensure!(
        saved_rows(w).await? == before,
        "mcp={mcp} {command}: a refusal changed state"
    );
    Ok(())
}

/// Every stored revision of one object, in revision order.
async fn object_history(w: &World, address: &str) -> Result<Value> {
    let conn = w.pool.get().await?;
    let rows: String = conn
        .query_one(
            "SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY r.revision),'[]')::text FROM joppa.object_revisions r WHERE workspace=$1 AND address=$2",
            &[&w.ws, &address],
        )
        .await?
        .get(0);
    Ok(serde_json::from_str(&rows)?)
}

/// The newest journal event: its actor and command.
async fn last_event(w: &World) -> Result<(String, Value)> {
    let conn = w.pool.get().await?;
    let row = conn
        .query_one(
            "SELECT actor, command::text FROM joppa.events WHERE workspace=$1 ORDER BY position DESC LIMIT 1",
            &[&w.ws],
        )
        .await?;
    Ok((row.get(0), serde_json::from_str(row.get(1))?))
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_releases_an_absent_workers_task_claim() -> Result<()> {
    // owner-releases-claim-ac1
    for mcp in [false, true] {
        let w = world().await?;
        let p = store::create_agent_token(&w.pool, &w.ws, "owner", "P", None).await?;
        let p = p["token"].as_str().unwrap();
        w.command_as(&w.bot, json!({"op":"assess","req":"requirement:key","revision":2,"ac":"ac:key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
        // Bot authors T. The human owner alex is neither the holder nor the workspace owner.
        let save = |revision: u32| json!({"op":"save_object","kind":"task","id":"t","revision":revision,"parent":"ac:key-rights","data":{"title":format!("Implement the specified work, draft {revision}"),"owner":"alex","contract":"Implement the criterion and record the result","purpose":"implementation","projects":[]},"reason":"Draft the work"});
        for revision in 0..7 {
            w.command_as(&w.bot, save(revision)).await?;
        }
        w.command_as(p, json!({"op":"claim_task","id":"task:t","revision":7}))
            .await?;
        let held = w.get("address=task:t").await?;
        ensure!(
            held["current_revision"] == 8
                && held["process"]["status"] == "in_progress"
                && held["process"]["assignee"] == "P"
                && held["document"]["data"]["owner"] == "alex",
            "{held}"
        );
        let release = |revision: u32, reason: &str| json!({"op":"release_task","id":"task:t","revision":revision,"reason":reason});

        refused_without_change(
            &w,
            mcp,
            &w.bot,
            release(8, "Worker unavailable"),
            "workspace owner required",
        )
        .await?;
        let after = w.get("address=task:t").await?;
        ensure!(
            after["current_revision"] == 8 && after["process"]["assignee"] == "P",
            "{after}"
        );
        refused_without_change(&w, mcp, &w.owner, release(8, " "), "text must not be empty")
            .await?;
        refused_without_change(
            &w,
            mcp,
            &w.owner,
            release(7, "Worker unavailable"),
            "stale Task revision; read the Task again",
        )
        .await?;

        let reply = send_over(&w, mcp, &w.owner, &release(8, "Worker unavailable")).await?;
        ensure!(reply.is_ok(), "mcp={mcp}: {reply:?}");
        let released = w.get("address=task:t").await?;
        ensure!(
            released["current_revision"] == 9
                && released["process"]["status"] == "open"
                && released["process"]["assignee"].is_null()
                && released["process"]["done"].is_null()
                && released["process"]["retired"].is_null()
                && released["document"]["actor"] == "owner"
                && released["document"]["reason"] == "Worker unavailable"
                && released["document"]["data"] == held["document"]["data"],
            "{released}"
        );
        let (actor, command) = last_event(&w).await?;
        ensure!(
            actor == "owner"
                && command["op"] == "release_task"
                && command["reason"] == "Worker unavailable",
            "{actor}: {command}"
        );
        let available = mcp_value(
            &w,
            "joppa_tasks",
            json!({"workspace":w.ws,"view":"available"}),
        )
        .await?;
        ensure!(
            available["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["address"] == "task:t" && row["claimable"] == true),
            "{available}"
        );
        refused_without_change(
            &w,
            mcp,
            &w.owner,
            release(9, "Worker unavailable"),
            "Task is not claimed unfinished work",
        )
        .await?;
    }
    Ok(())
}
