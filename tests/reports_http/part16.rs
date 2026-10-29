// Part 16 of tests/reports_http.rs, included there, so the test names stay unchanged.

// settings-agent-tokens-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn settings_agent_tokens_ac3_a_member_session_cannot_issue_or_revoke() -> Result<()> {
    let w = world().await?;
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let root = w.base.split("/api/").next().context("root")?.to_owned();
    let ws = w.base.rsplit('/').next().context("workspace")?.to_owned();
    // alex is a human member of W, not its owner, with a cookie session.
    let member: Operation =
        serde_json::from_value(json!({"op":"member","id":"alex","role":"human"}))?;
    store::execute(&pool, &ws, "owner", "ac3-member-alex", &member).await?;
    let session = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&pool, &session, "alex", None).await?;
    // The owner issues the token of bot.
    let (code, bot) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {bot}");
    let bot_id = bot["id"].as_i64().context("id")?;
    let token = bot["token"].as_str().context("token")?.to_owned();
    // Token and revocation rows of W.
    let counts = || async {
        let row = pool
            .get()
            .await?
            .query_one(
                "SELECT (SELECT count(*) FROM joppa.agent_tokens WHERE workspace=$1),
                        (SELECT count(*) FROM joppa.agent_token_revocations r
                         JOIN joppa.agent_tokens t ON t.id=r.token_id WHERE t.workspace=$1)",
                &[&ws],
            )
            .await?;
        anyhow::Ok((row.get::<_, i64>(0), row.get::<_, i64>(1)))
    };
    let before = counts().await?;
    ensure!(before == (1, 0), "{before:?}");

    // A same-origin create as alex is refused and stores no token.
    let (code, reply) = session_post(
        &w,
        &session,
        Some(&root),
        "/agent-tokens",
        json!({"agent":"helper","access":"read"}),
    )
    .await?;
    ensure!(
        code == 409 && reply["error"] == "workspace owner required",
        "{code} {reply}"
    );
    ensure!(counts().await? == before, "a member create stored data");

    // A same-origin revoke of the token of bot as alex is refused and stores no revocation.
    let (code, reply) = session_post(
        &w,
        &session,
        Some(&root),
        &format!("/agent-tokens/{bot_id}/revoke"),
        Value::Null,
    )
    .await?;
    ensure!(
        code == 409 && reply["error"] == "workspace owner required",
        "{code} {reply}"
    );
    ensure!(counts().await? == before, "a member revoke stored data");

    // The token of bot still reads W.
    let read = w
        .http
        .get(format!("{}/objects?parent=*", w.base))
        .bearer_auth(&token)
        .send()
        .await?;
    ensure!(read.status().is_success(), "{}", read.status());
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_token_does_not_reach_another_workspace() -> Result<()> {
    // agent-needs-grant-ac3 shape: the token makes the agent a member of one workspace only.
    let a = world().await?;
    let b = world().await?;
    let (_, created) = post(&a, &a.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    let bot = created["token"].as_str().unwrap().to_owned();
    let reply = b
        .http
        .get(format!("{}/objects?limit=5&kind=domain", b.base))
        .bearer_auth(&bot)
        .send()
        .await?;
    ensure!(reply.status().as_u16() == 404, "{}", reply.status());
    Ok(())
}

/// Call one MCP tool and return the raw JSON-RPC reply. Transport and decoding errors fail here.
async fn mcp(w: &World, token: &str, tool: &str, arguments: Value) -> Result<Value> {
    let root = w.base.split("/api/").next().unwrap();
    Ok(w
        .http
        .post(format!("{root}/mcp"))
        .bearer_auth(token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}}))
        .send()
        .await?
        .json()
        .await?)
}

/// A JSON-RPC result without isError=true gives its decoded text content.
fn read_result(tool: &str, reply: &Value) -> Result<Value> {
    let result = &reply["result"];
    ensure!(result.is_object(), "{tool}: no result: {reply}");
    let text = result["content"][0]["text"].as_str();
    ensure!(
        result["isError"] != true,
        "{tool}: {}",
        text.unwrap_or_default()
    );
    let text = text.ok_or_else(|| anyhow::anyhow!("{tool}: no text: {reply}"))?;
    Ok(serde_json::from_str(text)?)
}

async fn mcp_read(w: &World, token: &str, tool: &str, arguments: Value) -> Result<Value> {
    read_result(tool, &mcp(w, token, tool, arguments).await?)
}

/// Every object address of joppa_objects with parent="*", one item per page.
async fn all_objects(w: &World, token: &str) -> Result<(Vec<String>, usize)> {
    let workspace = w.base.rsplit('/').next().unwrap();
    let (mut addresses, mut pages, mut cursor) = (vec![], 0, None::<String>);
    loop {
        let mut arguments = json!({"workspace":workspace,"parent":"*","limit":1});
        if let Some(cursor) = &cursor {
            arguments["cursor"] = json!(cursor);
        }
        let page = mcp_read(w, token, "joppa_objects", arguments).await?;
        pages += 1;
        for item in page["items"].as_array().into_iter().flatten() {
            let address = item["address"].as_str();
            addresses.push(
                address
                    .ok_or_else(|| anyhow::anyhow!("no address: {item}"))?
                    .to_owned(),
            );
        }
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => return Ok((addresses, pages)),
        }
    }
}

/// The four reads of read-only-agent-token-ac1 with valid arguments; joppa_objects reads its first page.
fn four_reads(w: &World) -> [(&'static str, Value); 4] {
    let workspace = w.base.rsplit('/').next().unwrap();
    [
        (
            "joppa_objects",
            json!({"workspace":workspace,"parent":"*","limit":1}),
        ),
        (
            "joppa_tests",
            json!({"workspace":workspace,"ac":"ac:key-rights"}),
        ),
        ("joppa_tasks", json!({"workspace":workspace})),
        ("joppa_tree", json!({"workspace":workspace})),
    ]
}

async fn read_token(w: &World) -> Result<String> {
    let (code, created) = post(
        w,
        &w.owner,
        "/agent-tokens",
        json!({"agent":"planner-reader","access":"read"}),
    )
    .await?;
    ensure!(
        code == 200 && created["access"] == "read" && created["agent"] == "planner-reader",
        "{code} {created}"
    );
    Ok(created["token"].as_str().unwrap_or_default().to_owned())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_token_with_read_access_reads_the_workspace_as_the_owner() -> Result<()> {
    // read-only-agent-token-ac1: owner S issues a read token for planner-reader in W.
    let w = world().await?;
    let reader = read_token(&w).await?;
    // all_objects checks every joppa_objects page of the reader.
    let (read, pages) = all_objects(&w, &reader).await?;
    let (owned, _) = all_objects(&w, &w.owner).await?;
    ensure!(pages > 1, "joppa_objects needs more than one page: {pages}");
    ensure!(
        ["requirement:key", "ac:key-rights", "task:t1"]
            .iter()
            .all(|a| read.iter().any(|r| r == a)),
        "the fixture objects are listed: {read:?}"
    );
    ensure!(read == owned, "reader {read:?} owner {owned:?}");
    for (tool, arguments) in four_reads(&w).into_iter().skip(1) {
        mcp_read(&w, &reader, tool, arguments).await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_read_check_fails_for_a_read_token_without_membership() -> Result<()> {
    // read-only-agent-token-ac1 control: planner-reader is a member of B only, so reads in A fail.
    let a = world().await?;
    let b = world().await?;
    let outsider = read_token(&b).await?;
    for (tool, arguments) in four_reads(&a) {
        let reply = mcp(&a, &outsider, tool, arguments).await?;
        let result = &reply["result"];
        ensure!(
            result.is_object()
                && result["isError"] == true
                && result["content"][0]["text"] == "Workspace not found",
            "{tool} without membership: {reply}"
        );
        ensure!(
            read_result(tool, &reply).is_err(),
            "the read check accepts {tool}: {reply}"
        );
    }
    Ok(())
}

const NEEDS: &str = "acceptance needs records on the commit where every listed test passes";
