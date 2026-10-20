// Part 10 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_archived_capability_leaves_the_list_when_done_and_retired_tasks_remain() -> Result<()> {
    // removal-is-archiving-ac1
    let w = world().await?;
    let seed = json!({"op":"capability","id":"spent","title":"Finished work","domain":"security"});
    let seed: Operation = serde_json::from_value(seed)?;
    store::execute(&w.pool, &w.ws, "owner", "spent", &seed).await?;
    w.command(json!({"op":"revise","req":"spent","expected_revision":0,"capability":"spent","title":"Finished rule","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"spent-a","text":"Criterion A"}]})).await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"spent","revision":1,"ac":"spent-a","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let done = ready_task(&w, "spent-done", "spent-a").await?;
    w.command_as(&w.bot, json!({"op":"claim_task","id":done,"revision":1}))
        .await?;
    w.command_as(
        &w.bot,
        json!({"op":"complete_task","id":done,"revision":2,"evidence":"Implemented the contract"}),
    )
    .await?;
    let retired = ready_task(&w, "spent-retired", "spent-a").await?;
    w.command(json!({"op":"retire_task","id":retired,"revision":1,"reason":"The owner no longer needs this work"})).await?;
    for target in ["ac:spent-a", "requirement:spent", "capability:spent"] {
        let before = w.get(&format!("address={target}")).await?;
        w.command(json!({"op":"archive_object","target":target,"revision":before["current_revision"],"reason":"Remove the finished work"})).await?;
    }
    for task in [&done, &retired] {
        let read = w.get(&format!("address={task}")).await?;
        ensure!(read["document"]["present"] == true, "{read}");
    }
    let addresses = list_addresses(&w, &w.bot, "domain:security").await?;
    ensure!(
        addresses.contains(&"capability:restrict".to_string())
            && !addresses.contains(&"capability:spent".to_string()),
        "{addresses:?}"
    );
    let read = w.get_as(&w.bot, "address=capability:spent").await?;
    ensure!(read["document"]["present"] == false, "{read}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_requirement_with_no_live_criteria_archives_over_http() -> Result<()> {
    // removal-is-archiving-ac2: skopanev (the owner token) archives a requirement whose
    // criteria are all archived, and a requirement with no criteria.
    let w = world().await?;
    let before = w.get("address=ac:key-rights").await?;
    w.command(json!({"op":"archive_object","target":"ac:key-rights","revision":before["current_revision"],"reason":"Cut"})).await?;
    w.command(json!({"op":"save_object","kind":"requirement","id":"bare","revision":0,"parent":"capability:empty","data":{"title":"No criteria","body":"Scope","owner":"owner","consumers":["customer"]},"reason":"Define"})).await?;
    for target in ["requirement:key", "requirement:bare"] {
        let before = w.get(&format!("address={target}")).await?;
        let reply = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(&w.owner)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"archive_object","target":target,"revision":before["current_revision"],"reason":"Cut"}}))
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status == 200, "{target}: {status} {body}");
        let read = w.get(&format!("address={target}")).await?;
        ensure!(read["document"]["present"] == false, "{target}: {read}");
    }
    Ok(())
}

/// The journal entry at the `position` of a command reply, as `reader` reads it.
async fn journal_entry(w: &World, reader: &str, reply: &Value) -> Result<Value> {
    let position = &reply["position"];
    ensure!(
        position.is_u64(),
        "the reply has no journal position: {reply}"
    );
    let journal = store::read(&w.pool, &w.ws, reader, &store::Scope::full()).await?;
    let entries: Vec<&Value> = journal["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["position"] == *position)
        .collect();
    ensure!(entries.len() == 1, "{position}: {entries:?}");
    Ok(entries[0].clone())
}

/// The tree schema of the workspace, as the `token` account reads it over HTTP.
async fn read_tree(w: &World, token: &str) -> Result<Value> {
    let reply = w
        .http
        .get(format!("{}/api/workspaces/{}/tree", w.root, w.ws))
        .bearer_auth(token)
        .send()
        .await?;
    let status = reply.status();
    let body = reply.text().await?;
    ensure!(status.is_success(), "GET /tree: {status} {body}");
    Ok(serde_json::from_str(&body)?)
}

// removal-is-archiving-ac3
// skopanev creates and owns the workspace; alex is a human member of it.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_who_is_not_the_owner_archives_a_capability_over_http() -> Result<()> {
    let mut w = world().await?;
    w.ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Owned by skopanev"}),
        json!({"op":"member","id":"alex","role":"human"}),
        json!({"op":"domain","id":"security","title":"Security","authority":"skopanev"}),
        json!({"op":"capability","id":"restrict","title":"Restrict access","domain":"security"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "skopanev", &format!("setup-{i}"), &op).await?;
    }
    let cap = w.get_as(&w.alex, "address=capability:restrict").await?;
    let archive = json!({"op":"archive_object","target":"capability:restrict","revision":cap["current_revision"],"reason":"Cut"});
    // eve is not a member, so the access layer refuses her.
    let (ok, reply) = send_as(&w, &w.eve, &archive).await?;
    ensure!(!ok && reply["error"] == "Workspace not found", "{reply}");
    let (ok, reply) = send_as(&w, &w.alex, &archive).await?;
    ensure!(ok, "{reply}");
    let read = w.get_as(&w.alex, "address=capability:restrict").await?;
    ensure!(read["document"]["present"] == false, "{read}");
    let addresses = list_addresses(&w, &w.alex, "domain:security").await?;
    ensure!(
        !addresses.contains(&"capability:restrict".to_string()),
        "{addresses:?}"
    );
    let entry = journal_entry(&w, "skopanev", &reply).await?;
    ensure!(entry["actor"] == "alex", "{entry}");
    Ok(())
}

// tree-owner-sets-types-ac2
// skopanev creates and owns the workspace; alex is a human member of it.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_who_is_not_the_owner_changes_the_tree_over_http() -> Result<()> {
    let mut w = world().await?;
    w.ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Owned by skopanev"}),
        json!({"op":"member","id":"alex","role":"human"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "skopanev", &format!("setup-{i}"), &op).await?;
    }
    let before = read_tree(&w, &w.skopanev).await?;
    let n = before["current_revision"].as_u64();
    ensure!(n.is_some(), "{before}");
    let n = n.unwrap();
    let mut types = before["schema"]["types"].clone();
    ensure!(types.get("risk").is_none(), "{before}");
    types["risk"] = json!({"title":"Risk","role":"document","root":true,"parents":["*"]});
    let save = json!({"op":"set_tree_schema","revision":n,"types":types,"reason":"Add risk"});
    // eve is not a member, so the access layer refuses her.
    let (ok, reply) = send_as(&w, &w.eve, &save).await?;
    ensure!(!ok && reply["error"] == "Workspace not found", "{reply}");
    let (status, reply) =
        text_rule_request(&w, &w.alex, &uuid::Uuid::new_v4().to_string(), save, "http").await?;
    ensure!(status == 200, "{status} {reply}");
    let after = read_tree(&w, &w.skopanev).await?;
    ensure!(
        after["current_revision"] == n + 1 && after["schema"]["types"].get("risk").is_some(),
        "{after}"
    );
    let entry = journal_entry(&w, "skopanev", &reply).await?;
    ensure!(entry["actor"] == "alex", "{entry}");
    Ok(())
}

// bot is an agent member of the shared spec workspace. It has the rights of a member.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_member_changes_the_tree_over_http() -> Result<()> {
    let w = world().await?;
    let before = read_tree(&w, &w.bot).await?;
    let n = before["current_revision"].as_u64();
    ensure!(n.is_some(), "{before}");
    let n = n.unwrap();
    let mut types = before["schema"]["types"].clone();
    ensure!(types.get("risk").is_none(), "{before}");
    types["risk"] = json!({"title":"Risk","role":"document","root":false,"parents":["capability"],"references":{"consumers":{"target":"consumer","title":"Used by","multiple":true,"required":false,"verification_subject":false}}});
    let (status, reply) = text_rule_request(
        &w,
        &w.bot,
        &uuid::Uuid::new_v4().to_string(),
        json!({"op":"set_tree_schema","revision":n,"types":types,"reason":"Add risk"}),
        "http",
    )
    .await?;
    ensure!(status == 200, "{status} {reply}");
    let after = read_tree(&w, &w.bot).await?;
    ensure!(
        after["current_revision"] == n + 1 && after["schema"]["types"].get("risk").is_some(),
        "{after}"
    );
    let entry = journal_entry(&w, "owner", &reply).await?;
    ensure!(entry["actor"] == "bot", "{entry}");
    Ok(())
}

// removal-is-archiving-ac5
// skopanev owns the workspace and archives a capability with one live requirement.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_cannot_archive_a_capability_with_a_live_requirement_over_http() -> Result<()> {
    let mut w = world().await?;
    w.ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Owned by skopanev"}),
        json!({"op":"consumer","id":"customer","title":"Customer","owner":"skopanev"}),
        json!({"op":"domain","id":"security","title":"Security","authority":"skopanev"}),
        json!({"op":"capability","id":"restrict","title":"Restrict access","domain":"security"}),
        json!({"op":"save_object","kind":"requirement","id":"key","revision":0,"parent":"capability:restrict","data":{"title":"Key rights","body":"Scope","owner":"skopanev","consumers":["customer"]},"reason":"Define"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "skopanev", &format!("setup-{i}"), &op).await?;
    }
    let requirement = w.get_as(&w.skopanev, "address=requirement:key").await?;
    ensure!(requirement["document"]["present"] == true, "{requirement}");
    let cap = w.get_as(&w.skopanev, "address=capability:restrict").await?;
    let archive = json!({"op":"archive_object","target":"capability:restrict","revision":cap["current_revision"],"reason":"Cut"});
    let (ok, reply) = send_as(&w, &w.skopanev, &archive).await?;
    ensure!(
        !ok && reply["error"] == "cannot archive an object with active children",
        "{reply}"
    );
    let read = w.get_as(&w.skopanev, "address=capability:restrict").await?;
    ensure!(read["document"]["present"] == true, "{read}");
    Ok(())
}
