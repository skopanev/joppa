// Part 21 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot1_and_bot2_saves_record_their_own_journal_actor() -> Result<()> {
    // agent-own-identity-ac1
    let w = world().await?;
    let mut tokens = Vec::new();
    for agent in ["bot1", "bot2"] {
        let token = store::create_agent_token(&w.pool, &w.ws, "owner", agent, None).await?;
        let token = token["token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No token for {agent}: {token}"))?;
        tokens.push(token.to_string());
    }
    ensure!(tokens[0] != tokens[1], "bot1 and bot2 share a token");
    let mut saves = Vec::new();
    for (agent, token) in ["bot1", "bot2"].into_iter().zip(&tokens) {
        let request_id = uuid::Uuid::new_v4().to_string();
        let command = json!({"op":"save_object","kind":"note","id":format!("{agent}-note"),"revision":0,"parent":null,"data":{"title":format!("{agent} saves its own note"),"owner":"owner","body":"Each agent writes as itself"},"reason":"Record the agent author"});
        let response = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(token)
            .json(&json!({"request_id":request_id,"command":command}))
            .send()
            .await?;
        ensure!(
            response.status() == reqwest::StatusCode::OK,
            "{agent} request {request_id}: {}",
            response.status()
        );
        let saved: Value = response.json().await?;
        let position = saved["position"].as_u64().ok_or_else(|| {
            anyhow::anyhow!("{agent} request {request_id} returned no journal position: {saved}")
        })?;
        saves.push((agent, request_id, position));
    }
    ensure!(
        saves[0].1 != saves[1].1 && saves[0].2 != saves[1].2,
        "{saves:?}"
    );
    let conn = w.pool.get().await?;
    let journal = store::read(&w.pool, &w.ws, "owner", &store::Scope::full()).await?;
    for (agent, request_id, position) in &saves {
        let events = conn
            .query(
                "SELECT position,actor FROM joppa.events WHERE workspace=$1 AND request_id=$2",
                &[&w.ws, request_id],
            )
            .await?;
        let stored: Vec<(i64, String)> = events.iter().map(|e| (e.get(0), e.get(1))).collect();
        ensure!(
            stored.len() == 1 && stored[0].0 as u64 == *position && stored[0].1 == *agent,
            "{agent} request {request_id} at position {position} stored events {stored:?}"
        );
        let history: Vec<_> = journal["history"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("The journal has no history"))?
            .iter()
            .filter(|event| event["position"] == *position)
            .collect();
        ensure!(
            history.len() == 1
                && history[0]["actor"] == *agent
                && history[0]["actor_type"] == "agent",
            "{agent} request {request_id} at position {position} history events {history:?}"
        );
        for other in ["bot1", "bot2", "owner"]
            .into_iter()
            .filter(|other| other != agent)
        {
            ensure!(
                stored[0].1 != other && history[0]["actor"] != other,
                "{agent} request {request_id} names {other}: {stored:?} {history:?}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_question_reaches_its_named_person_and_keeps_the_input_open() -> Result<()> {
    // agent-asks-ac1
    async fn read(w: &World, address: &str, revision: Option<u32>) -> Result<Value> {
        let mut arguments = json!({"workspace":w.ws,"address":address});
        if let Some(revision) = revision {
            arguments["revision"] = json!(revision);
        }
        let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.maria).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":arguments}})).send().await?;
        ensure!(response.status().is_success(), "{}", response.status());
        let response: Value = response.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        Ok(serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?)
    }
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["input"] = types["note"].clone();
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check a question on a configured inbox type"})).await?;
    for kind in ["note", "input"] {
        let address = format!("{kind}:unclear");
        w.command_as(bot,json!({"op":"save_object","kind":kind,"id":"unclear","revision":0,"parent":null,"data":{"title":"Which plan needs this change","owner":"owner","body":"Clarify the plan before changing the specification","source":{"author":"alex"}},"reason":"Record an unclear input"})).await?;
        let command = json!({"op":"decide_note","target":address,"revision":1,"outcome":"needs_information","reason":"Ask the person who can clarify the plan","question":"Which plan?","recipient":"maria"});
        let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":command}}})).send().await?;
        ensure!(response.status().is_success(), "{}", response.status());
        let response: Value = response.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        let asked = read(&w, &address, None).await?;
        ensure!(
            asked["question"] == json!({"recipient":"maria","text":"Which plan?"}),
            "{asked}"
        );
        ensure!(
            asked["data"]["status"] == "unprocessed"
                && asked["data"]["source"]["author"] == "alex",
            "{asked}"
        );
        let response = w
            .http
            .get(format!("{}/api/workspaces/{}/objects", w.root, w.ws))
            .query(&[("address", address.as_str())])
            .bearer_auth(&w.maria)
            .send()
            .await?;
        ensure!(response.status().is_success(), "{}", response.status());
        let http = response.json::<Value>().await?;
        ensure!(
            crate::compact_of::compact_of(&http, &asked) == asked
                && http["question"] == asked["question"],
            "HTTP and MCP returned different questions"
        );
        let response = w.http.post(format!("{}/api/workspaces/{}/commands",w.root,w.ws)).bearer_auth(bot).json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"decide_note","target":address,"revision":2,"outcome":"needs_information","reason":"Test an invalid question recipient","question":"Which plan?","recipient":"bot"}})).send().await?;
        ensure!(
            response.status() == reqwest::StatusCode::CONFLICT,
            "{}",
            response.status()
        );
        ensure!(
            response.json::<Value>().await?["error"] == "a human member is required",
            "An agent was accepted as the named person"
        );
        ensure!(
            read(&w, &address, None).await?["data"] == asked["data"],
            "The refused recipient changed the input"
        );
        w.command_as(bot,json!({"op":"decide_note","target":address,"revision":2,"outcome":"changed_spec","reason":"The specification now states the plan","links":["ac:key-rights"]})).await?;
        let processed = read(&w, &address, None).await?;
        ensure!(
            processed["data"]["status"] == "processed"
                && processed.get("question").is_none(),
            "{processed}"
        );
        let old = read(&w, &address, Some(2)).await?;
        ensure!(
            old["question"] == asked["question"]
                && old["document"]["data"]["status"] == "unprocessed",
            "The saved question changed: {old}"
        );
        w.command_as(bot,json!({"op":"decide_note","target":address,"revision":3,"outcome":"needs_information","reason":"Keep the existing author routing","question":"Which plan?"})).await?;
        let default = read(&w, &address, None).await?;
        ensure!(
            default["question"]["recipient"] == "alex"
                && default["data"]["status"] == "unprocessed",
            "{default}"
        );
    }
    Ok(())
}
