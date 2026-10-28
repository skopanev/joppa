// Part 6 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_cannot_set_task_verification_by_hand() -> Result<()> {
    // done-is-not-green-ac2
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["job"] = types["task"].clone();
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check verification with a configured task type"})).await?;
    w.command_as(bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let conn = w.pool.get().await?;
    for kind in ["task", "job"] {
        w.command(json!({"op":"save_object","kind":kind,"id":"report-only","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the report contract","owner":"owner","contract":"Implement the key rights and record checks","purpose":"implementation","projects":[]},"reason":"Create work for the verification guard"})).await?;
        let address = format!("{kind}:report-only");
        w.command_as(bot, json!({"op":"claim_task","id":address,"revision":1}))
            .await?;
        let claimed = w.get(&format!("address={address}")).await?;
        ensure!(
            claimed["process"]["assignee"] == "bot"
                && claimed["process"].get("verification").is_none(),
            "{claimed}"
        );
        let before: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
                &[&w.ws],
            )
            .await?
            .get(0);
        let request_id = uuid::Uuid::new_v4().to_string();
        for verification in [json!("passing"), Value::Null] {
            let mut data = claimed["document"]["data"].clone();
            data["verification"] = verification;
            let command = json!({"op":"save_object","kind":kind,"id":"report-only","revision":claimed["current_revision"],"parent":"ac:key-rights","data":data,"reason":"Attempt to set verification by hand"});
            let response = w
                .http
                .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
                .bearer_auth(bot)
                .json(&json!({"request_id":request_id,"command":command}))
                .send()
                .await?;
            ensure!(
                response.status() == reqwest::StatusCode::CONFLICT,
                "{}",
                response.status()
            );
            let response: Value = response.json().await?;
            ensure!(
                response["error"] == "verification comes from test reports",
                "{response}"
            );
            let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":request_id,"command":command}}})).send().await?;
            ensure!(response.status().is_success(), "{}", response.status());
            let response: Value = response.json().await?;
            ensure!(
                response["result"]["isError"] == true
                    && response["result"]["content"][0]["text"]
                        == "verification comes from test reports",
                "{response}"
            );
        }
        let after: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
                &[&w.ws],
            )
            .await?
            .get(0);
        ensure!(
            before == after,
            "The refused verification write appended an event"
        );
        let receipts: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2",
                &[&w.ws, &request_id],
            )
            .await?
            .get(0);
        ensure!(
            receipts == 0,
            "The refused verification write consumed its request ID"
        );
        let unchanged = w.get(&format!("address={address}")).await?;
        ensure!(
            unchanged["document"] == claimed["document"]
                && unchanged["process"] == claimed["process"],
            "The refused verification write changed the task: {unchanged}"
        );
        let mut data = claimed["document"]["data"].clone();
        data["context"] = json!({"verification":"passing"});
        let valid = json!({"op":"save_object","kind":kind,"id":"report-only","revision":claimed["current_revision"],"parent":"ac:key-rights","data":data,"reason":"Keep a nested business value"});
        let response = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(bot)
            .json(&json!({"request_id":request_id,"command":valid}))
            .send()
            .await?;
        ensure!(response.status().is_success(), "{}", response.text().await?);
        let updated = w.get(&format!("address={address}")).await?;
        ensure!(
            updated["document"]["data"]["context"]["verification"] == "passing"
                && updated["process"].get("verification").is_none(),
            "{updated}"
        );
        w.command_as(bot, json!({"op":"release_task","id":address,"revision":updated["current_revision"],"reason":"Return the tested work"})).await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_cannot_supply_the_actor_of_a_write() -> Result<()> {
    // no-shared-credentials-ac1
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let error = "actor comes from the identity, not the request";
    let command = json!({"op":"save_object","kind":"note","id":"identity-guard","revision":0,"parent":null,"data":{"title":"Identity guard","owner":"owner","body":"Business context","actor":"business actor","source":{"actor":"skopanev"}},"reason":"Keep the authenticated author"});
    let request_id = uuid::Uuid::new_v4().to_string();
    let conn = w.pool.get().await?;
    let before: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    for claimed in [json!("skopanev"), Value::Null] {
        for in_command in [false, true] {
            let mut input = json!({"request_id":request_id,"command":command});
            if in_command {
                input["command"]["actor"] = claimed.clone();
            } else {
                input["actor"] = claimed.clone();
            }
            let response = w
                .http
                .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
                .bearer_auth(bot)
                .json(&input)
                .send()
                .await?;
            ensure!(
                response.status() == reqwest::StatusCode::UNPROCESSABLE_ENTITY,
                "{}: {}",
                response.status(),
                response.text().await?
            );
            let response: Value = response.json().await?;
            ensure!(response["error"] == error, "{response}");
        }
    }
    let mut nested = command.clone();
    nested["actor"] = json!("skopanev");
    let cases = [
        (
            "joppa_command",
            json!({"workspace":w.ws,"request_id":request_id,"actor":"skopanev","command":command}),
        ),
        (
            "joppa_command",
            json!({"workspace":w.ws,"request_id":request_id,"command":nested}),
        ),
        (
            "joppa_batch",
            json!({"workspace":w.ws,"request_id":request_id,"actor":"skopanev","operations":[{"command":command}]}),
        ),
        (
            "joppa_batch",
            json!({"workspace":w.ws,"request_id":request_id,"operations":[{"command":command},{"command":nested}]}),
        ),
        (
            "joppa_batch",
            json!({"workspace":w.ws,"request_id":request_id,"operations":[{"actor":"skopanev","command":command}]}),
        ),
    ];
    for (name, arguments) in cases {
        let expected = if name == "joppa_batch"
            && arguments["operations"][1]["command"].get("actor").is_some()
        {
            format!("operation 2: {error}")
        } else {
            error.to_owned()
        };
        let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":(crate::legacy_read_args(name, &arguments))}})).send().await?;
        ensure!(response.status().is_success(), "{}", response.status());
        let response: Value = response.json().await?;
        ensure!(
            response["result"]["isError"] == true
                && response["result"]["content"][0]["text"] == expected,
            "{response}"
        );
    }
    let after: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(before == after, "A refused write appended an event");
    let impersonated: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1 AND actor='skopanev'",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(impersonated == 0, "The bot token wrote as a person");
    let receipts: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2",
            &[&w.ws, &request_id],
        )
        .await?
        .get(0);
    ensure!(receipts == 0, "A refused write consumed its request ID");
    let saved = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(bot)
        .json(&json!({"request_id":request_id,"command":command}))
        .send()
        .await?;
    ensure!(saved.status().is_success(), "{}", saved.text().await?);
    let read = w.get("address=note:identity-guard").await?;
    ensure!(read["document"]["actor"] == "bot", "{read}");
    ensure!(
        read["document"]["data"]["actor"] == "business actor"
            && read["document"]["data"]["source"]["actor"] == "skopanev",
        "{read}"
    );
    Ok(())
}
