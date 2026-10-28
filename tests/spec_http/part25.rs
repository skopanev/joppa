// Part 25 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_read_only_agent_token_cannot_write() -> Result<()> {
    // read-only-agent-token-ac2
    let w = world().await?;
    /// One API POST; returns the status code and the JSON reply.
    async fn post(w: &World, token: &str, path: &str, body: &str) -> Result<(u16, Value)> {
        let reply = w
            .http
            .post(format!("{}/api/workspaces/{}{path}", w.root, w.ws))
            .bearer_auth(token)
            .header("content-type", "application/json")
            .body(body.to_owned())
            .send()
            .await?;
        let status = reply.status().as_u16();
        let text = reply.text().await?;
        Ok((status, serde_json::from_str(&text).unwrap_or(json!(text))))
    }

    // A claimable task, an object to comment on and the current tree schema revision.
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"claimable","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the key rights","owner":"owner","contract":"Implement the key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Create claimable work"})).await?;
    w.note("comment-target", 0, "Comment target", "Input", json!([]))
        .await?;
    let tree = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = tree["schema"]["types"].clone();
    types["job"] = types["task"].clone();
    types["job"]["title"] = json!("Job");

    // Owner S issues the read token and the write token for the control agent.
    let mut tokens = vec![];
    for (agent, access) in [("planner-reader", "read"), ("control-agent", "write")] {
        let (status, created) = post(
            &w,
            &w.owner,
            "/agent-tokens",
            &json!({"agent":agent,"access":access}).to_string(),
        )
        .await?;
        ensure!(
            status == 200 && created["access"] == access,
            "{status} {created}"
        );
        tokens.push(created["token"].as_str().unwrap().to_owned());
    }
    let (reader, writer) = (&tokens[0], &tokens[1]);

    let mcp_calls = [
        (
            "joppa_record_results",
            json!({"workspace":w.ws,"commit":"abcdef1","results":[{"test":"tests/spec_http.rs > spec > a_read_only_agent_token_cannot_write","status":"pass"}]}),
        ),
        (
            "joppa_command",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"save_object","kind":"note","id":"from-mcp","revision":0,"parent":null,"data":{"title":"Saved through MCP","owner":"owner","body":"Input"},"reason":"Save"}}),
        ),
        (
            "joppa_batch",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"operations":[{"command":{"op":"claim_task","id":"task:claimable","revision":1}}]}),
        ),
        (
            "joppa_command",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"note","target":"note:comment-target","id":"control","text":"A control comment"}}),
        ),
        (
            "joppa_command",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"set_tree_schema","revision":tree["schema"]["revision"],"types":types,"reason":"Add a job type"}}),
        ),
    ];
    let http_calls = [
        ("/blobs", format!("Read-only upload {}", uuid::Uuid::new_v4())),
        (
            "/agent-tokens",
            json!({"agent":"another-reader","access":"read"}).to_string(),
        ),
        (
            "/commands",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"save_object","kind":"note","id":"from-http","revision":0,"parent":null,"data":{"title":"Saved through HTTP","owner":"owner","body":"Input"},"reason":"Save"}}).to_string(),
        ),
    ];

    // Each read-token call is refused with the exact reply and stores nothing.
    let before = saved_rows(&w).await?;
    for (name, arguments) in &mcp_calls {
        let result = mcp_call(&w, reader, name, arguments.clone()).await?;
        ensure!(
            result == json!({"isError":true,"content":[{"type":"text","text":"read-only token"}]}),
            "{name} {arguments}: {result}"
        );
        ensure!(
            saved_rows(&w).await? == before,
            "{name} {arguments}: a refused tool stored data"
        );
    }
    for (path, body) in &http_calls {
        let (status, reply) = post(&w, reader, path, body).await?;
        ensure!(
            status == 403 && reply == json!({"error":"read-only token"}),
            "{path}: {status} {reply}"
        );
        ensure!(
            saved_rows(&w).await? == before,
            "{path}: a refused route stored data"
        );
    }

    // The same inputs succeed with a write grant, so each refusal comes from the grant.
    // Only the workspace owner sets the tree schema, so the owner sends that control.
    let mut last = before;
    for (name, arguments) in mcp_calls {
        let token = if arguments["command"]["op"] == "set_tree_schema" {
            &w.owner
        } else {
            writer
        };
        let result = mcp_call(&w, token, name, arguments.clone()).await?;
        ensure!(result["isError"] != true, "{name} {arguments}: {result}");
        let saved = saved_rows(&w).await?;
        ensure!(
            saved != last,
            "{name} {arguments}: the control stored nothing"
        );
        last = saved;
    }
    for (path, body) in http_calls {
        let token = if path == "/agent-tokens" {
            &w.owner
        } else {
            writer
        };
        let (status, reply) = post(&w, token, path, &body).await?;
        ensure!(status == 200, "{path}: {status} {reply}");
        let saved = saved_rows(&w).await?;
        ensure!(saved != last, "{path}: the control stored nothing");
        last = saved;
    }
    store::check_journal(&w.pool).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_token_without_access_writes_and_an_invalid_access_stores_nothing() -> Result<()> {
    let w = world().await?;
    let issue = |body: Value| {
        let w = &w;
        async move {
            let reply = w
                .http
                .post(format!("{}/api/workspaces/{}/agent-tokens", w.root, w.ws))
                .bearer_auth(&w.owner)
                .json(&body)
                .send()
                .await?;
            let code = reply.status().as_u16();
            let text = reply.text().await?;
            Ok::<_, anyhow::Error>((code, serde_json::from_str(&text).unwrap_or(json!(text))))
        }
    };
    let (code, created) = issue(json!({"agent":"bot"})).await?;
    ensure!(
        code == 200 && created["access"] == "write",
        "{code} {created}"
    );
    let token = created["token"].as_str().unwrap();
    ensure!(
        store::token_actor(&w.pool, token).await?.unwrap().access == store::Access::Write,
        "a token without access writes"
    );
    w.command_as(token, json!({"op":"save_object","kind":"note","id":"from-bot","revision":0,"parent":null,"data":{"title":"From bot","owner":"owner","body":"Input"},"reason":"Save"})).await?;
    let (code, created) = issue(json!({"agent":"bot","access":"write"})).await?;
    ensure!(
        code == 200 && created["access"] == "write",
        "{code} {created}"
    );

    let before = stored_counts(&w).await?;
    let (code, reply) = issue(json!({"agent":"stranger","access":"admin"})).await?;
    ensure!(code == 422, "{code} {reply}");
    let (code, reply) = issue(json!({"agent":"stranger","role":"read"})).await?;
    ensure!((400..500).contains(&code), "{code} {reply}");
    let refused = store::create_agent_token(&w.pool, &w.ws, "owner", "stranger", Some("admin"))
        .await
        .unwrap_err();
    ensure!(refused.is::<joppa::commands::ClientField>(), "{refused:#}");
    ensure!(
        stored_counts(&w).await? == before,
        "a refused issuance stored data"
    );
    let conn = w.pool.get().await?;
    let stranger: i64 = conn
        .query_one(
            "SELECT (SELECT count(*) FROM joppa.agent_tokens WHERE agent='stranger')
                  + (SELECT count(*) FROM joppa.state_entries WHERE section='members' AND key='stranger')",
            &[],
        )
        .await?
        .get(0);
    ensure!(stranger == 0, "the refused agent must not be stored");

    // A token row without access, as before the migration, reads as write and stays append-only.
    let raw = format!("jpt_{}", uuid::Uuid::new_v4().simple());
    let id: i64 = conn
        .query_one(
            "INSERT INTO joppa.agent_tokens(token_hash,agent,workspace,created_by) VALUES($1,'bot',$2,'owner') RETURNING id",
            &[&joppa::digest_hex::sha256_hex(raw.as_bytes()), &w.ws],
        )
        .await?
        .get(0);
    let grant = store::token_actor(&w.pool, &raw).await?.unwrap();
    ensure!(
        grant.agent == "bot" && grant.workspace == w.ws && grant.access == store::Access::Write,
        "{grant:?}"
    );
    let update = conn
        .execute(
            "UPDATE joppa.agent_tokens SET access='read' WHERE id=$1",
            &[&id],
        )
        .await;
    ensure!(update.is_err(), "agent tokens must stay append-only");
    Ok(())
}
