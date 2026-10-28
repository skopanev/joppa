// Part 15 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_without_membership_cannot_read_a_workspace_over_mcp() -> Result<()> {
    // agent-needs-grant-ac1
    let w = world().await?;
    let other = format!("http-spec-b-{}", uuid::Uuid::new_v4().simple());
    let secret = format!("private-body-{}", uuid::Uuid::new_v4().simple());
    for (index, command) in [
        json!({"op":"workspace","title":"Private workspace B"}),
        json!({"op":"save_object","kind":"note","id":"private","revision":0,"parent":null,"data":{"title":"Private note","owner":"someone-else","body":secret},"reason":"Test private workspace data"}),
    ]
    .into_iter()
    .enumerate()
    {
        let command: Operation = serde_json::from_value(command)?;
        store::execute(
            &w.pool,
            &other,
            "someone-else",
            &format!("private-{index}"),
            &command,
        )
        .await?;
    }
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let read = |workspace: String| {
        let w = &w;
        async move {
            let reply = w
                .http
                .post(format!("{}/mcp", w.root))
                .bearer_auth(bot)
                .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_read","arguments":{"verbose":true,"workspace":workspace}}}))
                .send()
                .await?;
            ensure!(reply.status().is_success(), "{}", reply.status());
            let reply: Value = reply.json().await?;
            ensure!(reply.get("error").is_none(), "{reply}");
            Ok::<_, anyhow::Error>(reply["result"].clone())
        }
    };
    let allowed = read(w.ws.clone()).await?;
    ensure!(allowed["isError"] != true, "{allowed}");
    let allowed: Value = serde_json::from_str(
        allowed["content"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("The allowed read has no document"))?,
    )?;
    ensure!(allowed["workspace"] == w.ws, "{allowed}");
    let existing = read(other).await?;
    let absent = read(format!("absent-{}", uuid::Uuid::new_v4().simple())).await?;
    ensure!(existing == absent, "{existing} vs {absent}");
    for denied in [existing, absent] {
        ensure!(denied["isError"] == true, "{denied}");
        ensure!(
            denied["content"] == json!([{"type":"text","text":"Workspace not found"}]),
            "{denied}"
        );
        ensure!(
            !denied.to_string().contains(&secret),
            "The refused read exposed the private note"
        );
        ensure!(
            denied.get("structuredContent").is_none(),
            "The refused read included document data: {denied}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_required_source_refuses_an_agent_write_without_a_source() -> Result<()> {
    // signal-records-source-ac2
    let w = world().await?;
    w.note("archived", 0, "Archived input", "No source", json!([]))
        .await?;
    w.command(json!({"op":"archive_object","target":"note:archived","revision":1,"reason":"Archive before requiring a source"})).await?;
    w.note("original", 0, "Original input", "Keep this text", json!([]))
        .await?;
    let original = w.get("address=note:original").await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["note"]["required_fields"] = json!({"source":"a signal needs a source"});
    types["feedback"] = json!({"title":"Feedback","role":"document","status":"none","root":true,"parents":["*"],"required_fields":{"source":"a signal needs a source"}});
    types["record"] = json!({"title":"Record","role":"document","status":"none","root":true,"parents":["*"]});
    let schema_command = json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Require the configured source field"});
    let error = w.refused(schema_command.clone()).await?;
    ensure!(error.contains("a signal needs a source"), "{error}");
    let after = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    ensure!(after == schema, "A refused schema changed: {after}");
    ensure!(
        w.get("address=note:original").await?["document"] == original["document"],
        "A refused schema changed the original input"
    );
    let source = json!({"channel":"web","author":"alex","at":"2026-10-09T10:00:00Z"});
    let mut data = original["document"]["data"].clone();
    data["source"] = source.clone();
    w.command(json!({"op":"save_object","kind":"note","id":"original","revision":1,"parent":null,"data":data,"reason":"Supply the source before enabling the rule"})).await?;
    w.command(schema_command).await?;
    let archived = w.pool.get().await?.query_one("SELECT present, document::text FROM joppa.objects WHERE workspace=$1 AND address='note:archived'", &[&w.ws]).await?;
    let present: bool = archived.get(0);
    let document: Value = serde_json::from_str(archived.get(1))?;
    ensure!(
        !present && document["data"].get("source").is_none(),
        "An archived input must not block or receive the required field: {document}"
    );
    let history = w.get("address=note:original&revision=1").await?;
    ensure!(
        history["document"] == original["document"],
        "The source rule changed a historical revision: {history}"
    );
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = grant["token"].as_str().unwrap();
    for kind in ["note", "feedback"] {
        for transport in ["http", "mcp"] {
            let id = format!("{kind}-{transport}");
            let address = format!("{kind}:{id}");
            let request_id = uuid::Uuid::new_v4().to_string();
            let mut command = json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":null,"data":{"title":"New input","owner":"owner","body":"Input text"},"reason":"Check the configured source rule"});
            let conn = w.pool.get().await?;
            let before: i64 = conn
                .query_one(
                    "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
                    &[&w.ws],
                )
                .await?
                .get(0);
            let request = |command: Value| {
                let w = &w;
                let request_id = &request_id;
                async move {
                    let (url, body) = if transport == "http" {
                        (
                            format!("{}/api/workspaces/{}/commands", w.root, w.ws),
                            json!({"request_id":request_id,"command":command}),
                        )
                    } else {
                        (
                            format!("{}/mcp", w.root),
                            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":request_id,"command":command}}}),
                        )
                    };
                    let response = w.http.post(url).bearer_auth(bot).json(&body).send().await?;
                    Ok::<_, anyhow::Error>((
                        response.status().as_u16(),
                        response.json::<Value>().await?,
                    ))
                }
            };
            let (status, denied) = request(command.clone()).await?;
            if transport == "http" {
                ensure!(
                    status == 422 && denied["error"] == "a signal needs a source",
                    "{status}: {denied}"
                );
            } else {
                ensure!(
                    status == 200
                        && denied["result"]["isError"] == true
                        && denied["result"]["content"][0]["text"] == "a signal needs a source",
                    "{status}: {denied}"
                );
            }
            let after: i64 = conn
                .query_one(
                    "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
                    &[&w.ws],
                )
                .await?
                .get(0);
            ensure!(after == before, "A refused write appended an event");
            let revisions: i64 = conn
                .query_one(
                    "SELECT COUNT(*) FROM joppa.object_revisions WHERE workspace=$1 AND address=$2",
                    &[&w.ws, &address],
                )
                .await?
                .get(0);
            ensure!(revisions == 0, "A refused write created a revision");
            let receipts: i64 = conn.query_one("SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2", &[&w.ws, &request_id]).await?.get(0);
            ensure!(receipts == 0, "A refused write consumed its request ID");
            drop(conn);
            command["data"]["source"] = source.clone();
            let (status, saved) = request(command).await?;
            ensure!(status == 200, "{status}: {saved}");
            if transport == "mcp" {
                ensure!(saved["result"]["isError"] != true, "{saved}");
            }
            let read = w.get(&format!("address={address}")).await?;
            ensure!(
                read["document"]["data"]["source"] == source && read["current_revision"] == 1,
                "{read}"
            );
        }
    }
    let error = w.refused(json!({"op":"save_object","kind":"note","id":"original","revision":2,"parent":null,"data":original["document"]["data"],"reason":"Try to remove the required source"})).await?;
    ensure!(error.contains("a signal needs a source"), "{error}");
    let original_after = w.get("address=note:original").await?;
    ensure!(
        original_after["current_revision"] == 2
            && original_after["document"]["data"]["source"] == source,
        "{original_after}"
    );
    w.command_as(bot, json!({"op":"save_object","kind":"record","id":"plain","revision":0,"parent":null,"data":{"title":"Ordinary record","owner":"owner"},"reason":"The schema does not require source for this type"})).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_description_without_given_is_refused() -> Result<()> {
    configured_description_without_marker_is_refused("Given:").await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_description_without_when_is_refused() -> Result<()> {
    configured_description_without_marker_is_refused("When:").await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_description_without_then_is_refused() -> Result<()> {
    configured_description_without_marker_is_refused("Then:").await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_description_without_evidence_is_refused() -> Result<()> {
    configured_description_without_marker_is_refused("Evidence:").await
}
