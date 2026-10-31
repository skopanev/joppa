// Part 16 of tests/spec_http.rs, included there, so the test names stay unchanged.

async fn text_rule_request(
    w: &World,
    token: &str,
    request_id: &str,
    command: Value,
    transport: &str,
) -> Result<(u16, Value)> {
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
    let response = w
        .http
        .post(url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;
    Ok((response.status().as_u16(), response.json().await?))
}

async fn text_rule_refusal(
    w: &World,
    token: &str,
    request_id: &str,
    command: Value,
    error: &str,
) -> Result<()> {
    let conn = w.pool.get().await?;
    let sql = "SELECT (SELECT COUNT(*) FROM joppa.events WHERE workspace=$1), (SELECT COUNT(*) FROM joppa.object_revisions WHERE workspace=$1), (SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2)";
    let counts = || async {
        let row = conn.query_one(sql, &[&w.ws, &request_id]).await?;
        Ok::<_, anyhow::Error>((
            row.get::<_, i64>(0),
            row.get::<_, i64>(1),
            row.get::<_, i64>(2),
        ))
    };
    let before = counts().await?;
    ensure!(before.2 == 0, "The request ID already has a receipt");
    for transport in ["http", "mcp"] {
        let (status, denied) =
            text_rule_request(w, token, request_id, command.clone(), transport).await?;
        if transport == "http" {
            ensure!(
                status == 422 && denied["error"] == error,
                "{status}: {denied}"
            );
        } else {
            ensure!(
                status == 200
                    && denied["result"]["isError"] == true
                    && denied["result"]["content"][0]["text"] == error,
                "{status}: {denied}"
            );
        }
        ensure!(
            counts().await? == before,
            "A refused write stored an event, revision or receipt"
        );
    }
    Ok(())
}

async fn configured_description_without_marker_is_refused(missing: &str) -> Result<()> {
    let w = world().await?;
    let markers = ["Given:", "When:", "Then:", "Evidence:"];
    let valid = "Given: an input.\nWhen: the input changes.\nThen: the contract changes.\nEvidence: the saved contract.";
    let invalid = markers
        .iter()
        .filter(|marker| **marker != missing)
        .map(|marker| format!("{marker} a check."))
        .collect::<Vec<_>>()
        .join("\n");
    let error = "an ac description needs Given:, When:, Then: and Evidence:";
    let criterion = w.get("address=ac:key-rights").await?;
    let mut data = criterion["document"]["data"].clone();
    data["body"] = json!(valid);
    w.command(json!({"op":"save_object","kind":"ac","id":"key-rights","revision":criterion["current_revision"],"parent":criterion["document"]["parent"],"data":data,"reason":"Prepare existing criterion text"})).await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["observation"] =
        json!({"title":"Observation","role":"document","status":"none","root":true,"parents":["*"]});
    types["record"] = json!({"title":"Record","role":"document","status":"none","root":true,"parents":["*"]});
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Add ordinary document types"})).await?;
    for id in ["original", "archived"] {
        w.command(json!({"op":"save_object","kind":"observation","id":id,"revision":0,"parent":null,"data":{"title":"An observation","owner":"owner","body":"Original text"},"reason":"Keep original text"})).await?;
    }
    let original = w.get("address=observation:original").await?;
    w.command(json!({"op":"archive_object","target":"observation:archived","revision":1,"reason":"Archive before the new rule"})).await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    let question = "Which one check proves the rule?";
    let rule = json!({"contains":markers,"error":error});
    for kind in ["ac", "observation"] {
        types[kind]["question"] = json!(question);
        types[kind]["required_fields"] = json!({"body":rule});
    }
    if missing == "Given:" {
        types["domain"]["required_fields"] = json!({"title":{"contains":["Sec","urity"],"error":"A domain title needs both markers"}});
    }
    let command = json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Require literal text markers"});
    let schema_request = uuid::Uuid::new_v4().to_string();
    text_rule_refusal(&w, &w.owner, &schema_request, command.clone(), error).await?;
    ensure!(
        store::tree_schema(&w.pool, &w.ws, "owner", None).await? == schema,
        "A refused schema changed"
    );
    let mut data = original["document"]["data"].clone();
    data["body"] = json!(valid);
    w.command(json!({"op":"save_object","kind":"observation","id":"original","revision":1,"parent":null,"data":data,"reason":"Prepare current text before the rule"})).await?;
    let (status, saved) = text_rule_request(&w, &w.owner, &schema_request, command, "http").await?;
    ensure!(status == 200, "{status}: {saved}");
    ensure!(
        w.get("address=observation:original&revision=1").await?["document"] == original["document"],
        "The rule changed historical text"
    );
    let archived = w.get("address=observation:archived").await?;
    ensure!(
        archived["document"]["present"] == false
            && archived["document"]["data"]["body"] == "Original text",
        "The rule changed archived data: {archived}"
    );
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = grant["token"].as_str().unwrap();
    for kind in ["ac", "observation"] {
        let parent = if kind == "ac" {
            json!("requirement:key")
        } else {
            Value::Null
        };
        let mut command = json!({"op":"save_object","kind":kind,"id":"new","revision":0,"parent":parent,"data":{"title":"The input changes the contract","owner":"owner","body":invalid},"reason":"Check the configured text rule"});
        if kind == "ac" {
            command["data"]["consumer"] = json!("customer");
        }
        let request_id = uuid::Uuid::new_v4().to_string();
        if missing == "Given:" {
            let question_error = format!(
                "Check that the title and body answer: {question}. Rewrite them if they do not. Then resend with question_checked"
            );
            text_rule_refusal(&w, bot, &request_id, command.clone(), &question_error).await?;
            command["question_checked"] = json!("A different question");
            text_rule_refusal(&w, bot, &request_id, command.clone(), &question_error).await?;
        }
        command["question_checked"] = json!(question);
        text_rule_refusal(&w, bot, &request_id, command.clone(), error).await?;
        command["data"]["body"] = json!(valid);
        let transport = if kind == "ac" { "http" } else { "mcp" };
        let (status, saved) =
            text_rule_request(&w, bot, &request_id, command.clone(), transport).await?;
        ensure!(status == 200, "{status}: {saved}");
        if transport == "mcp" {
            ensure!(saved["result"]["isError"] != true, "{saved}");
        }
        let address = format!("{kind}:new");
        let saved = w.get(&format!("address={address}")).await?;
        ensure!(
            saved["current_revision"] == 1
                && command["data"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .all(|(field, value)| saved["document"]["data"].get(field) == Some(value)),
            "{saved}"
        );
        command["revision"] = json!(1);
        command["data"]["body"] = json!(invalid);
        text_rule_refusal(&w, bot, &uuid::Uuid::new_v4().to_string(), command, error).await?;
        ensure!(
            w.get(&format!("address={address}")).await?["document"] == saved["document"],
            "The refused update changed the document"
        );
    }
    w.command_as(bot, json!({"op":"save_object","kind":"record","id":"plain","revision":0,"parent":null,"data":{"title":"An ordinary record","owner":"owner","body":"No markers"},"reason":"Keep a type without a rule unchanged"})).await?;
    if missing == "Given:" {
        text_rule_refusal(
            &w,
            bot,
            &uuid::Uuid::new_v4().to_string(),
            json!({"op":"save_object","kind":"domain","id":"legacy","revision":0,"parent":null,"data":{"title":"No markers","owner":"owner"},"reason":"Setup"}),
            "A domain title needs both markers",
        )
        .await?;
        w.command_as(
            bot,
            json!({"op":"save_object","kind":"domain","id":"legacy","revision":0,"parent":null,"data":{"title":"Security","owner":"owner"},"reason":"Setup"}),
        )
        .await?;
        ensure!(w.get("address=domain:legacy").await?["document"]["data"]["title"] == "Security");
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn legacy_document_commands_obey_configured_required_fields() -> Result<()> {
    let w = world().await?;
    let domain = w.get("address=domain:security").await?;
    let mut data = domain["document"]["data"].clone();
    data["origin"] = json!("The agreed security scope");
    w.command(json!({"op":"save_object","kind":"domain","id":"security","revision":domain["current_revision"],"parent":null,"data":data,"reason":"Supply the origin before enabling the rule"})).await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["domain"]["required_fields"] = json!({"origin":"origin is required"});
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Apply the rule to legacy document writes"})).await?;
    let conn = w.pool.get().await?;
    let before: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    let error = w.refused(json!({"op":"save_object","kind":"domain","id":"no-origin","revision":0,"parent":null,"data":{"title":"A domain without an origin","owner":"owner"},"reason":"Setup"})).await?;
    let error: Value = serde_json::from_str(&error)?;
    ensure!(error["error"] == "origin is required", "{error}");
    let after: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(
        before == after,
        "The refused legacy command appended an event"
    );
    let revisions: i64 = conn.query_one("SELECT COUNT(*) FROM joppa.object_revisions WHERE workspace=$1 AND address='domain:no-origin'", &[&w.ws]).await?.get(0);
    ensure!(
        revisions == 0,
        "The refused legacy command created a revision"
    );
    Ok(())
}
