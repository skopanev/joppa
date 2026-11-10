// Part 40 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// One joppa_command of `token` in workspace `ws` over MCP. A refusal gives Err with its text.
async fn mcp_command(
    w: &World,
    token: &str,
    ws: &str,
    command: &Value,
) -> Result<Result<Value, String>> {
    let id = uuid::Uuid::new_v4().to_string();
    let args = json!({"workspace":ws,"request_id":id,"command":command});
    let result = mcp_call(w, token, "joppa_command", args).await?;
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    Ok(if result["isError"] == true {
        Err(text.to_owned())
    } else {
        Ok(serde_json::from_str(text)?)
    })
}

/// The actor and the position of the newest journal entry of `ws`, as alex reads it.
async fn newest(w: &World, ws: &str) -> Result<(Value, u64)> {
    let args = json!({"workspace":ws,"order":"newest","limit":1});
    let result = mcp_call(w, &w.alex, "joppa_history", args).await?;
    ensure!(result["isError"] != true, "{result}");
    let text = result["content"][0]["text"].as_str().unwrap_or("null");
    let page: Value = serde_json::from_str(text)?;
    let item = &page["items"][0];
    let position = item["position"]
        .as_u64()
        .with_context(|| format!("{page}"))?;
    Ok((item["actor"].clone(), position))
}

/// alex sends `command` in `ws`, and then `newest` reads it in `read`. The command succeeds,
/// its event names alex, and the journal position grows. Gives the new position.
async fn alex_runs(w: &World, ws: &str, read: &str, command: &Value, position: u64) -> Result<u64> {
    let reply = mcp_command(w, &w.alex, ws, command).await?;
    reply.map_err(|e| anyhow::anyhow!("{command}: {e}"))?;
    let (actor, now) = newest(w, read).await?;
    ensure!(actor == "alex", "{command}: the event names {actor}");
    ensure!(now > position, "{command}: position {position} -> {now}");
    Ok(now)
}

/// A save_object of `address` in `ws` with a new title and the other fields, parent and data
/// it has.
async fn retitle(w: &World, ws: &str, address: &str, title: &str) -> Result<Value> {
    let url = format!("{}/api/workspaces/{ws}/objects?address={address}", w.root);
    let reply = w.http.get(url).bearer_auth(&w.owner).send().await?;
    ensure!(reply.status().is_success(), "{address}: {}", reply.status());
    let current: Value = reply.json().await?;
    let (kind, id) = address.split_once(':').context("address")?;
    let mut data = current["document"]["data"].clone();
    data["title"] = json!(title);
    Ok(
        json!({"op":"save_object","kind":kind,"id":id,"revision":current["current_revision"],
        "parent":current["document"]["parent"],"data":data,"reason":"New title"}),
    )
}

/// The full read of `ws`: members, objects, title and positions.
async fn whole(w: &World, ws: &str) -> Result<Value> {
    store::read(&w.pool, ws, "owner", &store::Scope::full()).await
}

// requirement:member-rights: a human member who did not create the workspace runs each former
// owner command over MCP, and an agent keeps its refusals.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_human_member_who_did_not_create_the_workspace_runs_each_former_owner_command()
-> Result<()> {
    let w = world().await?;
    // bob (actor owner) created W with the confirmed domain security, the confirmed capability
    // restrict and the consumer customer. alex is a human member. bot is an agent.
    w.note("secret", 0, "Secret note", "Token s3cr3t", json!([]))
        .await?;
    w.note("other", 0, "Other note", "Key k3y", json!([]))
        .await?;
    let ws = w.ws.clone();
    let mut position = newest(&w, &ws).await?.1;
    let mut commands = vec![
        json!({"op":"member","id":"newcomer","role":"human"}),
        json!({"op":"remove_member","id":"newcomer","reason":"Remove access"}),
        json!({"op":"redact_object","target":"note:secret","fragment":"s3cr3t","reason":"Remove the secret"}),
    ];
    for address in [
        "capability:restrict",
        "domain:security",
        "consumer:customer",
    ] {
        commands.push(retitle(&w, &ws, address, &format!("{address} by alex")).await?);
    }
    for command in &commands {
        position = alex_runs(&w, &ws, &ws, command, position).await?;
    }
    // alex changes the title, the workspace_id and the reference_prefix last.
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let renamed = format!("member-rights-{suffix}");
    let rename = json!({"op":"update_workspace","title":"Renamed by alex","workspace_id":renamed,
        "reference_prefix":format!("mr-{suffix}"),"reason":"Rename"});
    position = alex_runs(&w, &ws, &renamed, &rename, position).await?;
    let whole_after = whole(&w, &renamed).await?;
    let after = whole_after.to_string();
    ensure!(after.contains("Renamed by alex"), "the title of alex");
    // The journal keeps the commands as sent; the stored object holds no fragment.
    let secret = whole_after["state"]["objects"]["note:secret"].to_string();
    ensure!(
        secret.contains("[redacted]") && !secret.contains("s3cr3t"),
        "the redaction of alex: {secret}"
    );
    ensure!(
        after.contains("capability:restrict by alex"),
        "the save of alex"
    );
    // The former owner commands of bot. A node save needs no human member, as before: after
    // migrate_documents a capability, a domain and a consumer save like any node.
    let human = "a human member of the workspace is required";
    let refused = vec![
        (
            json!({"op":"member","id":"newcomer2","role":"human"}),
            human,
        ),
        (
            json!({"op":"remove_member","id":"maria","reason":"Remove access"}),
            human,
        ),
        (
            json!({"op":"redact_object","target":"note:other","fragment":"k3y","reason":"Remove the key"}),
            human,
        ),
        (
            json!({"op":"update_workspace","title":"Bot title","reason":"Rename"}),
            "a human member updates the workspace",
        ),
    ];
    // bot sends the same commands with its credential. Each one is refused, and W does not change.
    let before = whole(&w, &renamed).await?;
    for (command, text) in &refused {
        let reply = mcp_command(&w, &w.bot, &renamed, command).await?;
        let refusal = reply
            .err()
            .with_context(|| format!("{command} was accepted"))?;
        ensure!(refusal.contains(text), "{command}: {refusal}");
        ensure!(
            !refusal.contains("workspace owner") && !refusal.contains("owner required"),
            "{command}: {refusal}"
        );
    }
    ensure!(
        whole(&w, &renamed).await? == before,
        "a refusal of bot changed W"
    );
    ensure!(
        newest(&w, &renamed).await?.1 == position,
        "a refusal of bot moved the journal"
    );
    Ok(())
}
