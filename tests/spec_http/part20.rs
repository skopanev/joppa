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
        first["address"] == "note:second"
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
        first["address"] == "note:second"
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
async fn the_agent_bot_finds_an_object_first_by_its_generated_local_id() -> Result<()> {
    let w = world().await?;
    w.note("k3x9m2p7", 0, "Passkey plan", "passkey for first", json!([]))
        .await?;
    w.note("other", 0, "Passkey rollout", "passkey after k3x9m2p7", json!([]))
        .await?;
    let found = mcp_value(&w, "joppa_search", json!({"workspace":w.ws,"query":"k3x9m2p7"})).await?;
    let items = found["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        items.len() == 1 && items[0]["address"] == "note:k3x9m2p7" && items[0]["exact_id"] == true,
        "{found}"
    );
    // A query of another form searches the text: both notes match.
    let words = mcp_value(&w, "joppa_search", json!({"workspace":w.ws,"query":"passkey"})).await?;
    ensure!(words["items"].as_array().map(Vec::len) == Some(2), "{words}");
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

