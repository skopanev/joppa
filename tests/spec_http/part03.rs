// Part 3 of tests/spec_http.rs, included there, so the test names stay unchanged.

// [ac:completion-record-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn completing_a_task_stores_exact_evidence_and_bot_event_actor() -> Result<()> {
    let w = world().await?;
    let read = |address: &str| {
        let w = &w;
        let arguments = json!({"workspace":w.ws,"address":address});
        async move {
            let result = mcp_call(w, &w.bot, "joppa_objects", arguments).await?;
            ensure!(result["isError"] != true, "{result}");
            Ok::<Value, anyhow::Error>(serde_json::from_str(
                result["content"][0]["text"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("MCP result needs text"))?,
            )?)
        }
    };
    let address = "task:recorded";
    let evidence = "commit abc123 on master";
    w.command_as(&w.bot, json!({"op":"assess","req":"requirement:key","revision":2,"ac":"ac:key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"recorded","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the specified work","owner":"owner","body":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Check the completion record"})).await?;
    w.command_as(&w.bot, json!({"op":"claim_task","id":address,"revision":1}))
        .await?;
    let claimed = read(address).await?;
    ensure!(
        claimed["process"]["status"] == "in_progress" && claimed["process"]["assignee"] == "bot",
        "{claimed}"
    );
    w.command_as(&w.bot, json!({"op":"complete_task","commit":"c0ffee0","id":address,"revision":claimed["revision"],"evidence":evidence})).await?;
    let tool = read(address).await?;
    ensure!(
        tool["process"]["status"] == "done" && tool["process"]["done"] == evidence,
        "{tool}"
    );
    let http: Value = w
        .http
        .get(format!("{}/api/workspaces/{}/objects", w.root, w.ws))
        .bearer_auth(&w.bot)
        .query(&[("address", address)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(
        crate::compact_of::compact_of(&http, &tool) == tool,
        "HTTP and MCP disagree: {http} / {tool}"
    );
    let events = w
        .pool
        .get()
        .await?
        .query(
            "SELECT actor FROM joppa.events WHERE workspace=$1 AND command->>'op'='complete_task' AND command->>'id'=$2",
            &[&w.ws, &address],
        )
        .await?;
    let actors: Vec<String> = events.iter().map(|row| row.get(0)).collect();
    ensure!(actors == ["bot"], "one bot completion event: {actors:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_account_without_membership_cannot_upload_a_file() -> Result<()> {
    // files-keep-versions-ac2
    let w = world().await?;
    w.note(
        "file-target",
        0,
        "Private files",
        "Workspace members only",
        json!([]),
    )
    .await?;
    let bytes = format!("Unaccepted file {}", uuid::Uuid::new_v4());
    let hash = joppa::digest_hex::sha256_hex(bytes.as_bytes());
    let before = w.get("address=note:file-target").await?;
    ensure!(before["files"] == json!([]), "{before}");
    let mut replies = Vec::new();
    for workspace in [
        w.ws.clone(),
        format!("absent-{}", uuid::Uuid::new_v4().simple()),
    ] {
        let response = w
            .http
            .post(format!("{}/api/workspaces/{workspace}/blobs", w.root))
            .bearer_auth(&w.eve)
            .body(bytes.clone())
            .send()
            .await?;
        let status = response.status().as_u16();
        let body: Value = response.json().await?;
        ensure!(
            status == 404 && body == json!({"error":"Workspace not found"}),
            "{status}: {body}"
        );
        replies.push((status, body));
        let after = w.get("address=note:file-target").await?;
        ensure!(
            after == before,
            "A refused upload changed the object: {after}"
        );
        ensure!(
            !fake_spaces::start().any(&hash),
            "A refused upload stored file content"
        );
    }
    ensure!(
        replies[0] == replies[1],
        "Upload replies expose workspace existence: {replies:?}"
    );
    Ok(())
}
