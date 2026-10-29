// Part 20 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_item_id_and_an_address_find_their_object_first() -> Result<()> {
    // lookup-by-id-ac1, lookup-by-id-ac2
    let w = world().await?;
    w.note("first", 0, "Passkey plan", "passkey for first", json!([]))
        .await?;
    w.note(
        "second",
        0,
        "Passkey rollout",
        "passkey for second",
        json!([]),
    )
    .await?;
    let item_id = w.get("address=note:second").await?["item_id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    ensure!(!item_id.is_empty(), "the note needs an item ID");
    let by_id = w.first_exact(&item_id).await?;
    ensure!(
        by_id.as_deref() == Some("note:second"),
        "{item_id}: {by_id:?}"
    );
    let by_address = w.first_exact("note:second").await?;
    ensure!(
        by_address.as_deref() == Some("note:second"),
        "{by_address:?}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_agent_bot_finds_an_object_first_by_its_current_and_kept_item_ids() -> Result<()> {
    // lookup-by-id-ac1
    let w = world().await?;
    w.note("first", 0, "Passkey plan", "passkey for first", json!([]))
        .await?;
    w.note(
        "second",
        0,
        "Passkey rollout",
        "passkey for second",
        json!([]),
    )
    .await?;
    let current = w.get("address=note:second").await?["item_id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    ensure!(!current.is_empty(), "the note needs an item ID");
    bot_finds_first(&w, &current).await?;
    let prefix = format!("renamed-{}", uuid::Uuid::new_v4().simple());
    w.command(json!({"op":"rename_workspace","title":"Spec over HTTP","reason":"New prefix","reference_prefix":prefix}))
        .await?;
    let renamed = w.get("address=note:second").await?["item_id"].clone();
    ensure!(
        renamed.as_str().is_some_and(|id| id.starts_with(&prefix)),
        "{renamed}"
    );
    bot_finds_first(&w, &current).await
}

/// The agent bot's search for an item ID returns only note:second, with that ID as an exact match.
async fn bot_finds_first(w: &World, query: &str) -> Result<()> {
    let found = mcp_value(w, "joppa_search", json!({"workspace":w.ws,"query":query})).await?;
    let first = &found["items"][0];
    ensure!(
        first["kind"] == "note"
            && first["id"] == "second"
            && first.get("system_id").is_none()
            && first["exact_id"] == true
            && found["items"].as_array().map(Vec::len) == Some(1),
        "{query}: {found}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_agent_bot_finds_an_object_first_by_its_address() -> Result<()> {
    // lookup-by-id-ac2
    let w = world().await?;
    w.note("first", 0, "Passkey plan", "passkey for first", json!([]))
        .await?;
    w.note(
        "second",
        0,
        "Passkey rollout",
        "passkey for second",
        json!([]),
    )
    .await?;
    let query = "note:second";
    let found = mcp_value(&w, "joppa_search", json!({"workspace":w.ws,"query":query})).await?;
    let first = &found["items"][0];
    ensure!(
        first["kind"] == "note"
            && first["id"] == "second"
            && first["exact_id"] == true
            && found["items"].as_array().map(Vec::len) == Some(1),
        "{query}: {found}"
    );
    for (name, arguments) in [
        ("joppa_search", json!({"workspace":w.ws,"query":query})),
        ("joppa_objects", json!({"workspace":w.ws,"address":query})),
    ] {
        let denied = mcp_call(&w, &w.eve, name, arguments).await?;
        ensure!(
            denied["isError"] == true && !denied.to_string().contains("Passkey rollout"),
            "{name} {query}: {denied}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_signal_keeps_its_channel_author_and_time() -> Result<()> {
    // signal-records-source-ac1
    let w = world().await?;
    let source = json!({"channel_type":"chat","channel_name":"support","author":"maria","at":"2026-10-09T10:00:00Z"});
    w.command_as(&w.bot, json!({"op":"save_object","kind":"note","id":"chat","revision":0,"parent":null,"data":{"title":"Refunds are slow","owner":"owner","body":"My refund takes a week","source":source},"links":[],"reason":"Intake"}))
        .await?;
    let read = w.get("address=note:chat").await?;
    ensure!(read["document"]["data"]["source"] == source, "{read}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_granted_agent_write_records_its_own_journal_identity() -> Result<()> {
    // agent-acts-within-grant-ac1
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let request_id = uuid::Uuid::new_v4().to_string();
    let command = json!({"op":"save_object","kind":"note","id":"agent-author","revision":0,"parent":null,"data":{"title":"The granted agent writes as itself","owner":"owner","body":"The owner and author have separate meanings","source":{"actor":"business source"}},"reason":"Record the authenticated agent author"});
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(bot)
        .json(&json!({"request_id":request_id,"command":command}))
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::OK,
        "{}",
        response.status()
    );
    let saved: Value = response.json().await?;
    let position = saved["position"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("The write returned no journal position: {saved}"))?;
    let conn = w.pool.get().await?;
    let events = conn.query("SELECT position,actor,command::text FROM joppa.events WHERE workspace=$1 AND request_id=$2", &[&w.ws,&request_id]).await?;
    ensure!(events.len() == 1, "The write has {} events", events.len());
    ensure!(
        events[0].get::<_, i64>(0) as u64 == position && events[0].get::<_, String>(1) == "bot",
        "The saved event has a different position or author"
    );
    let stored: Value = serde_json::from_str(events[0].get(2))?;
    ensure!(
        stored["op"] == "save_object" && stored["kind"] == "note" && stored["id"] == "agent-author",
        "{stored}"
    );
    let journal = store::read(&w.pool, &w.ws, "owner", &store::Scope::full()).await?;
    let events: Vec<_> = journal["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["position"] == position)
        .collect();
    ensure!(
        events.len() == 1,
        "The journal lost or duplicated the write"
    );
    ensure!(
        events[0]["actor"] == "bot" && events[0]["actor_type"] == "agent",
        "{:?}",
        events[0]
    );
    let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":w.ws,"address":"note:agent-author"}}})).send().await?;
    ensure!(
        response.status() == reqwest::StatusCode::OK,
        "{}",
        response.status()
    );
    let response: Value = response.json().await?;
    ensure!(response["result"]["isError"] != true, "{response}");
    let object: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("null"),
    )?;
    ensure!(
        object["document"]["actor"] == "bot" && object["document"]["actor_type"] == "agent",
        "{object}"
    );
    ensure!(
        object["document"]["data"]["owner"] == "owner"
            && object["document"]["data"]["source"]["actor"] == "business source",
        "The identity changed business data: {object}"
    );
    Ok(())
}
