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
