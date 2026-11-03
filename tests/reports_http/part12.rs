// Part 12 of tests/reports_http.rs, included there, so the test names stay unchanged.

async fn post(w: &World, token: &str, path: &str, body: Value) -> Result<(u16, Value)> {
    let reply = w
        .http
        .post(format!("{}{path}", w.base))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;
    Ok((
        reply.status().as_u16(),
        reply.json().await.unwrap_or(Value::Null),
    ))
}

async fn save_note(w: &World, token: &str, id: &str) -> Result<(u16, Value)> {
    let command = json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"owner","body":"Signal"},"links":[],"reason":"Save"});
    post(
        w,
        token,
        "/commands",
        json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}),
    )
    .await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_token_writes_as_the_agent_and_stops_when_revoked() -> Result<()> {
    // agent-own-identity-ac1, write-records-agent-ac1, removal-at-once-ac2
    let w = world().await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(
        code == 200
            && created["token"]
                .as_str()
                .is_some_and(|t| t.starts_with("jpt_")),
        "{code} {created}"
    );
    let bot = created["token"].as_str().unwrap().to_owned();
    let (code, saved) = save_note(&w, &bot, "from-bot").await?;
    ensure!(code == 200, "{code} {saved}");
    let read: Value = w
        .http
        .get(format!("{}/objects?address=note:from-bot", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    ensure!(
        read["document"]["actor"] == "bot",
        "the write records the agent: {read}"
    );
    let id = created["id"].as_i64().unwrap();
    let (code, _) = post(
        &w,
        &w.owner,
        &format!("/agent-tokens/{id}/revoke"),
        json!({}),
    )
    .await?;
    ensure!(code == 200, "revoke");
    let (code, reply) = save_note(&w, &bot, "after-revoke").await?;
    ensure!(
        code == 401 && reply["error"] == "token revoked",
        "{code} {reply}"
    );
    Ok(())
}
