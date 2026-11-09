// Part 9 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_account_without_membership_cannot_comment_on_an_object() -> Result<()> {
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    let question = "Which fact does this record preserve?";
    types["record"] = json!({"title":"Record","role":"document","status":"none","root":true,"parents":[],"question":question});
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Use a configured document type"})).await?;
    w.command(json!({"op":"save_object","kind":"record","id":"one","revision":0,"parent":null,"data":{"title":"A recorded fact","owner":"owner","body":"The current boundary"},"reason":"Prepare the comment target"})).await?;
    let absent = format!("absent-{}", uuid::Uuid::new_v4().simple());
    let workspaces = vec![w.ws.clone(), absent.clone()];
    let conn = w.pool.get().await?;
    let counts = || async {
        let row = conn.query_one("SELECT (SELECT COUNT(*) FROM joppa.events WHERE workspace=ANY($1)), (SELECT COUNT(*) FROM joppa.object_revisions WHERE workspace=ANY($1)), (SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=ANY($1))", &[&workspaces]).await?;
        Ok::<_, anyhow::Error>((
            row.get::<_, i64>(0),
            row.get::<_, i64>(1),
            row.get::<_, i64>(2),
        ))
    };
    for address in ["domain:security", "record:one"] {
        let original = w.get(&format!("address={address}")).await?;
        let before = counts().await?;
        let command =
            json!({"op":"note","target":address,"id":"outsider","text":"An unauthorized comment"});
        for transport in ["http", "mcp"] {
            let request_id = uuid::Uuid::new_v4().to_string();
            let (status, denied) =
                text_rule_request(&w, &w.eve, &request_id, command.clone(), transport).await?;
            let (url, body) = if transport == "http" {
                (
                    format!("{}/api/workspaces/{absent}/commands", w.root),
                    json!({"request_id":request_id,"command":command}),
                )
            } else {
                (
                    format!("{}/mcp", w.root),
                    json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":absent,"request_id":request_id,"command":command}}}),
                )
            };
            let response = w
                .http
                .post(url)
                .bearer_auth(&w.eve)
                .json(&body)
                .send()
                .await?;
            let absent_status = response.status().as_u16();
            let absent_reply: Value = response.json().await?;
            ensure!(
                status == absent_status && denied == absent_reply,
                "The refusal reveals workspace access: {status}: {denied} vs {absent_status}: {absent_reply}"
            );
            if transport == "http" {
                ensure!(
                    status == 404 && denied["error"] == "Workspace not found",
                    "{status}: {denied}"
                );
            } else {
                ensure!(
                    status == 200
                        && denied["result"]["isError"] == true
                        && denied["result"]["content"][0]["text"] == "Workspace not found",
                    "{status}: {denied}"
                );
            }
            ensure!(
                counts().await? == before,
                "The refused comment stored an event, revision or receipt"
            );
        }
        let after = w.get(&format!("address={address}")).await?;
        ensure!(
            after["document"] == original["document"]
                && after["annotations"]["comments"] == original["annotations"]["comments"],
            "The refused comment changed the object: {after}"
        );
        w.command_as(
            &w.alex,
            json!({"op":"note","target":address,"id":"member","text":"A member comment"}),
        )
        .await?;
        let current = w.get(&format!("address={address}")).await?;
        ensure!(
            current["annotations"]["comments"]["member"]["author"] == "alex"
                && current["annotations"]["comments"]["member"]["text"] == "A member comment",
            "{current}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn any_past_revision_reads_whole_by_its_number() -> Result<()> {
    // past-revision-whole-ac1
    let w = world().await?;
    let first = json!({"title":"A","owner":"owner","body":"first text","status":"unprocessed"});
    for (revision, data) in [
        first.clone(),
        json!({"title":"B","owner":"owner","body":"second text","status":"unprocessed"}),
        json!({"title":"C","owner":"owner","body":"third text","status":"unprocessed"}),
    ]
    .into_iter()
    .enumerate()
    {
        w.command(json!({"op":"save_object","kind":"note","id":"a","revision":revision,"parent":null,"data":data,"reason":"Save"})).await?;
    }
    let reply = w
        .http
        .get(format!("{}/api/workspaces/{}/objects", w.root, w.ws))
        .bearer_auth(&w.bot)
        .query(&[("address", "note:a"), ("revision", "1")])
        .send()
        .await?;
    let status = reply.status();
    let text = reply.text().await?;
    ensure!(status.is_success(), "{status}: {text}");
    let old: Value = serde_json::from_str(&text)?;
    ensure!(
        old["document"]["revision"] == 1 && old["document"]["data"] == first,
        "{old}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_archived_object_leaves_the_list_of_its_parent() -> Result<()> {
    // removal-is-archiving-ac1
    let w = world().await?;
    let cap = w.get("address=capability:empty").await?;
    w.command(json!({"op":"archive_object","target":"capability:empty","revision":cap["document"]["revision"],"reason":"Cut"})).await?;
    let addresses = list_addresses(&w, &w.bot, "domain:security").await?;
    ensure!(
        addresses.contains(&"capability:restrict".to_string())
            && !addresses.contains(&"capability:empty".to_string()),
        "{addresses:?}"
    );
    let read = w.get_as(&w.bot, "address=capability:empty").await?;
    ensure!(read["document"]["present"] == false, "{read}");
    Ok(())
}
