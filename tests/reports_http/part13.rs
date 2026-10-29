// Part 13 of tests/reports_http.rs, included there, so the test names stay unchanged.

// removal-at-once-ac2
// Owner S revokes the write token of bot. The first call after that is a read, and it fails.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_revoked_write_token_fails_on_the_first_read_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let workspace = w.base.rsplit('/').next().unwrap().to_owned();
    let root = w.base.split("/api/").next().unwrap().to_owned();
    let read_http = |token: String| {
        let request = w
            .http
            .get(format!("{}/objects?address=note:from-bot", w.base))
            .bearer_auth(token);
        async move {
            let reply = request.send().await?;
            anyhow::Ok((
                reply.status().as_u16(),
                reply.json::<Value>().await.unwrap_or(Value::Null),
            ))
        }
    };
    let read_mcp = |token: String| {
        let request = w.http.post(format!("{root}/mcp")).bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":workspace,"address":"note:from-bot"}}}));
        async move {
            let reply = request.send().await?;
            anyhow::Ok((
                reply.status().as_u16(),
                reply.json::<Value>().await.unwrap_or(Value::Null),
            ))
        }
    };
    for mcp in [false, true] {
        let (code, created) = post(
            &w,
            &w.owner,
            "/agent-tokens",
            json!({"agent":"bot","access":"write"}),
        )
        .await?;
        ensure!(
            code == 200 && created["access"] == "write" && created["agent"] == "bot",
            "{code} {created}"
        );
        let bot = created["token"].as_str().unwrap_or_default().to_owned();
        // Controls: the write grant writes, and the token reads.
        if !mcp {
            let (code, saved) = save_note(&w, &bot, "from-bot").await?;
            ensure!(code == 200, "{code} {saved}");
        }
        let (code, read) = read_http(bot.clone()).await?;
        ensure!(
            code == 200 && read["address"] == "note:from-bot",
            "{code} {read}"
        );
        let (code, read) = read_mcp(bot.clone()).await?;
        ensure!(
            code == 200 && read["result"]["isError"] != true,
            "{code} {read}"
        );
        let id = created["id"].as_i64().unwrap_or_default();
        let (code, reply) = post(
            &w,
            &w.owner,
            &format!("/agent-tokens/{id}/revoke"),
            json!({}),
        )
        .await?;
        ensure!(code == 200, "revoke: {code} {reply}");
        let (code, reply) = if mcp {
            read_mcp(bot).await?
        } else {
            read_http(bot).await?
        };
        ensure!(
            code == 401 && reply["error"] == "token revoked",
            "first read after the revocation, MCP={mcp}: {code} {reply}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn only_the_owner_creates_an_agent_token() -> Result<()> {
    // removal-at-once-ac3 shape: a member who is not the owner cannot manage tokens.
    let w = world().await?;
    let (code, reply) = post(&w, &w.token, "/agent-tokens", json!({"agent":"bot2"})).await?;
    ensure!(
        code == 409 && reply["error"] == "workspace owner required",
        "{code} {reply}"
    );
    Ok(())
}

// read-only-agent-token-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_token_list_shows_the_access_of_each_token() -> Result<()> {
    // read-only-agent-token-ac3: the list shows read for planner-reader and write for bot.
    let w = world().await?;
    let (code, reader) = post(
        &w,
        &w.owner,
        "/agent-tokens",
        json!({"agent":"planner-reader","access":"read"}),
    )
    .await?;
    ensure!(
        code == 200
            && reader["access"] == "read"
            && reader["token"]
                .as_str()
                .is_some_and(|t| t.starts_with("jpt_")),
        "{code} {reader}"
    );
    let (code, bot) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200 && bot["access"] == "write", "{code} {bot}");
    let list = |token: &str| {
        w.http
            .get(format!("{}/agent-tokens", w.base))
            .bearer_auth(token)
            .send()
    };
    let reply = list(&w.owner).await?;
    ensure!(reply.status().as_u16() == 200, "{}", reply.status());
    let listed: Value = reply.json().await?;
    // No token or token hash among the fields.
    let fields = [
        "access",
        "agent",
        "created_at",
        "id",
        "revoked",
        "revoked_at",
    ];
    for (created, access) in [(&reader, "read"), (&bot, "write")] {
        let agent = &created["agent"];
        let item = listed["items"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|i| &i["agent"] == agent)
            .ok_or_else(|| anyhow::anyhow!("{agent} not listed: {listed}"))?;
        let mut keys: Vec<&str> = item
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, _)| k.as_str())
            .collect();
        keys.sort_unstable();
        ensure!(
            item["access"] == access
                && item["id"] == created["id"]
                && item["created_at"].is_string()
                && item["revoked"] == false
                && item["revoked_at"].is_null()
                && keys == fields,
            "{item}"
        );
    }
    let reply = list(&w.token).await?;
    let code = reply.status().as_u16();
    let refused: Value = reply.json().await?;
    ensure!(
        code == 409 && refused["error"] == "workspace owner required",
        "{code} {refused}"
    );
    Ok(())
}

/// A cookie-session token call to the workspace of `w`, with an optional Origin header.
async fn session_post(
    w: &World,
    session: &str,
    origin: Option<&str>,
    path: &str,
    body: Value,
) -> Result<(u16, Value)> {
    let mut request = w
        .http
        .post(format!("{}{path}", w.base))
        .header("cookie", format!("joppa_session={session}"))
        .json(&body);
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    let reply = request.send().await?;
    Ok((
        reply.status().as_u16(),
        reply.json().await.unwrap_or(Value::Null),
    ))
}
