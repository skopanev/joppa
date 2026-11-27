// Part 3 of tests/p1_feedback_http.rs: ac:ytqs5gst.

// ac:ytqs5gst
// test:cpjaj19f
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p1_10_owner_me_persists_member_id_and_help_and_refusals_name_fixes() -> Result<()> {
    let w = record_world::world(&[]).await?;
    w.command(
        &w.owner,
        json!({
            "op": "save_object",
            "kind": "requirement",
            "id": "me_req",
            "revision": 0,
            "parent": null,
            "data": {"title": "Me Req", "body": "Body", "owner": "me"},
            "reason": "Human owner me"
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    let card = mcp_ok(
        &w,
        &w.bot,
        "joppa_objects",
        json!({"address":"requirement:me_req","fields":["owner"]}),
    )
    .await?;
    ensure!(card["owner"] == "owner", "card owner: {card}");

    let event_owner: Option<String> = w
        .pool
        .get()
        .await?
        .query_one(
            "SELECT command->'data'->>'owner' FROM joppa.events WHERE workspace=$1 ORDER BY position DESC LIMIT 1",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(
        event_owner.as_deref() == Some("owner"),
        "journal stored {event_owner:?}"
    );
    replay(&w).await?;
    let after_replay = mcp_ok(
        &w,
        &w.bot,
        "joppa_objects",
        json!({"address":"requirement:me_req","fields":["owner"]}),
    )
    .await?;
    ensure!(after_replay["owner"] == "owner", "replay owner mismatch: {after_replay}");

    let bot_me = save(
        &w,
        json!({"op":"save_object","kind":"requirement","id":"bot_me","revision":0,"parent":null,"data":{"title":"T","body":"B","owner":"me"},"reason":"Bot me"}),
    )
    .await?;
    refused(
        bot_me,
        "owner \"me\" requires a human member caller, pass a human member ID",
    )?;

    let bad_kind = save(
        &w,
        json!({"op":"save_object","kind":"req","id":"x","revision":0,"parent":null,"data":{"title":"T","owner":"owner"},"reason":"Bad kind"}),
    )
    .await?;
    refused(
        bad_kind,
        "unknown object type req: valid types are ac, requirement, test",
    )?;

    for args in [
        json!({"address":"req:x"}),
        json!({"address":"req:x","revision":1}),
    ] {
        let bad_read = w.mcp(&w.bot, "joppa_objects", args).await?;
        refused(
            bad_read,
            "unknown object: req:x (unknown type req, valid types are ac, requirement, test)",
        )?;
    }
    let missing_known = w
        .mcp(&w.bot, "joppa_objects", json!({"address":"ac:missing"}))
        .await?;
    ensure!(
        missing_known == Err("unknown object: ac:missing".into()),
        "{missing_known:?}"
    );

    let help_save = mcp_ok(&w, &w.bot, "joppa_help", json!({"op":"save_object"})).await?;
    let data_help = help_save["operations"]["save_object"]["optional"]["data"]
        .as_str()
        .unwrap_or_default();
    ensure!(
        data_help.contains("data.title")
            && data_help.contains("data.owner")
            && data_help.contains("\"me\"")
            && !data_help.contains(';'),
        "data_help: {data_help}"
    );

    let help_objects = mcp_ok(&w, &w.bot, "joppa_help", json!({"section":"objects"})).await?;
    let docs_text = help_objects["objects"].as_str().unwrap_or_default();
    for needle in ["request_id", "replayed", "dry_run", "latest_wins"] {
        ensure!(docs_text.contains(needle), "objects help missing {needle}");
    }

    let raw = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await?
        .json::<Value>()
        .await?;
    let batch_tool = raw["result"]["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| t["name"] == "joppa_batch")
        .context("joppa_batch")?;
    let defaults_desc = batch_tool["inputSchema"]["properties"]["defaults"]["description"]
        .as_str()
        .unwrap_or_default();
    ensure!(
        defaults_desc.contains("without op"),
        "defaults_desc: {defaults_desc}"
    );
    Ok(())
}
