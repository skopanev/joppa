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
                && history[0].get("actor_type").is_none(),
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
