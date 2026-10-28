// Part 8 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// P (bot) holds T with two notes and a record at def456. P or the workspace owner
/// releases T; Q (bot2) claims it and reads the same notes, commit and earlier revisions.
async fn returned_work_handover(owner_release: bool) -> Result<()> {
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot2", None).await?;
    let bot2 = token["token"].as_str().unwrap();
    let notes = [
        ("first", "The first boundary is clear"),
        ("second", "The second boundary still needs work"),
    ];
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"handover","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the key contract","owner":"owner","contract":"Implement the exact key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Create work for the handover"})).await?;
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"task:handover","revision":1}),
    )
    .await?;
    for (id, text) in notes {
        w.command_as(
            &w.bot,
            json!({"op":"note","target":"task:handover","id":id,"text":text}),
        )
        .await?;
    }
    // Check runs are internal now: the fixture records the partial run through the store.
    let internal = async |actor: &str, command: Value| {
        store::execute_input(&w.pool, &w.ws, actor, &uuid::Uuid::new_v4().to_string(), &command).await
    };
    internal("owner", json!({"op":"define_verification_scope","req":"key","revision":1,"subject_keys":["commit"],"scope":"The key rights","reason":"Record the exact partial commit"})).await?;
    internal("bot", json!({"op":"plan","req":"key","revision":1,"run":"partial","subject":{"commit":"def456"},"scope":"The key rights","discovery_complete":false,"checks":[{"id":"[key-rights] first-boundary","ac":"key-rights","method":"automated","expected":["The first boundary holds"],"tasks":[{"address":"task:handover","origin_revision":1}]},{"id":"[key-rights] second-boundary","ac":"key-rights","method":"automated","expected":["The second boundary holds"],"tasks":[{"address":"task:handover","origin_revision":1}]}]})).await?;
    let partial = json!({"check":"[key-rights] first-boundary","outcome":"pass","observed":["The first boundary holds"],"note":"Partial implementation at commit def456","link":"https://example.test/repository/commit/def456"});
    internal(
        "bot",
        json!({"op":"result","req":"key","revision":1,"run":"partial","evidence":partial}),
    )
    .await?;

    let before = object_as(&w, bot2, "task:handover", None).await?;
    let n = before["current_revision"].as_u64().unwrap() as u32;
    let mut history = Vec::new();
    for revision in 1..=n {
        history
            .push(object_as(&w, bot2, "task:handover", Some(revision)).await?["document"].clone());
    }
    let (releaser, actor, reason) = if owner_release {
        (&w.owner, "owner", "The worker is unavailable")
    } else {
        (&w.bot, "bot", "Return the remaining boundary to the queue")
    };
    w.command_as(
        releaser,
        json!({"op":"release_task","id":"task:handover","revision":n,"reason":reason}),
    )
    .await?;
    let available = mcp_call(
        &w,
        bot2,
        "joppa_tasks",
        json!({"workspace":w.ws,"view":"available"}),
    )
    .await?;
    ensure!(available["isError"] != true, "{available}");
    let available: Value =
        serde_json::from_str(available["content"][0]["text"].as_str().unwrap_or("null"))?;
    ensure!(
        available["tasks"]
            .as_array()
            .is_some_and(|tasks| tasks.iter().any(|task| task["address"] == "task:handover")),
        "{available}"
    );
    w.command_as(
        bot2,
        json!({"op":"claim_task","id":"task:handover","revision":n + 1}),
    )
    .await?;

    let task = object_as(&w, bot2, "task:handover", None).await?;
    ensure!(task["process"]["assignee"] == "bot2", "{task}");
    ensure!(task["process"]["status"] == "in_progress", "{task}");
    ensure!(
        task["document"]["data"] == before["document"]["data"],
        "the handover changed the document: {task}"
    );
    let comments = &task["annotations"]["comments"];
    for (id, text) in notes {
        let note = &before["annotations"]["comments"][id];
        ensure!(
            comments[id] == *note,
            "note {id} changed: {note} became {}",
            comments[id]
        );
        ensure!(
            note["author"] == "bot" && note["text"] == text,
            "note {id} lost its author or text: {note}"
        );
    }
    ensure!(
        task["annotations"] == before["annotations"],
        "the handover changed the original notes: {task}"
    );

    ensure!(
        task["current_revision"] == n + 2,
        "expected revision {} after the release and claim: {task}",
        n + 2
    );
    for (revision, captured) in (1..=n).zip(&history) {
        let read = object_as(&w, bot2, "task:handover", Some(revision)).await?;
        ensure!(
            read["document"] == *captured,
            "revision {revision} changed: {captured} became {}",
            read["document"]
        );
    }
    let released = object_as(&w, bot2, "task:handover", Some(n + 1)).await?["document"].clone();
    ensure!(
        released["actor"] == actor
            && released["reason"] == reason
            && released["data"] == before["document"]["data"],
        "revision {} is not the release: {released}",
        n + 1
    );
    let claimed = object_as(&w, bot2, "task:handover", Some(n + 2)).await?["document"].clone();
    ensure!(
        claimed["actor"] == "bot2"
            && claimed["reason"] == "Task claimed"
            && claimed["data"] == before["document"]["data"],
        "revision {} is not the claim by bot2: {claimed}",
        n + 2
    );

    let evidence = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/board?level=process&id=task:handover",
            w.root, w.ws
        ))
        .bearer_auth(bot2)
        .send()
        .await?;
    ensure!(evidence.status().is_success(), "{}", evidence.text().await?);
    let evidence: Value = evidence.json().await?;
    let run = &evidence["runs"]["partial"];
    ensure!(
        run["subject"]["commit"] == "def456" && run["reporter"] == "bot",
        "{run}"
    );
    ensure!(
        run["finished"].is_null() && run["discovery_complete"] == false,
        "{run}"
    );
    ensure!(
        run["results"]["[key-rights] first-boundary"] == partial
            && run["results"]["[key-rights] second-boundary"].is_null(),
        "{run}"
    );
    ensure!(
        run["plan"][0]["tasks"][0]["address"] == "task:handover",
        "{run}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_comment_records_its_author_and_server_time() -> Result<()> {
    // comments-on-object-ac1
    let w = world().await?;
    let clock = w.pool.get().await?;
    let server_time = || async {
        let time: String = clock
            .query_one("SELECT clock_timestamp()::text", &[])
            .await?
            .get(0);
        Ok::<_, anyhow::Error>(
            chrono::DateTime::parse_from_str(&time, "%Y-%m-%d %H:%M:%S%.f%#z")?
                .with_timezone(&chrono::Utc),
        )
    };
    w.note("input", 0, "Input", "Context", json!([])).await?;
    for address in [
        "domain:security",
        "capability:restrict",
        "requirement:key",
        "ac:key-rights",
        "note:input",
    ] {
        let before = server_time().await?;
        let response = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(&w.alex)
            .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"note","target":address,"id":"n1","text":"Clarify the boundary"}}))
            .send()
            .await?;
        ensure!(response.status().is_success(), "{}", response.text().await?);
        let object = w.get(&format!("address={address}")).await?;
        let comment = &object["annotations"]["comments"]["n1"];
        ensure!(comment["author"] == "alex", "{comment}");
        ensure!(comment["actor_type"] == "person", "{comment}");
        ensure!(comment["actor"] == comment["author"], "{comment}");
        ensure!(comment["at"] == comment["created_at"], "{comment}");
        let created = chrono::DateTime::parse_from_str(
            comment["created_at"].as_str().unwrap_or(""),
            "%Y-%m-%d %H:%M:%S%.f%#z",
        )?
        .with_timezone(&chrono::Utc);
        let after = server_time().await?;
        ensure!(
            created >= before && created <= after,
            "{comment}; server interval {before} to {after}"
        );
        ensure!(
            (created - before).num_milliseconds() <= 5000,
            "comment time exceeds the five second write interval: {comment}"
        );
        let response = w
            .http
            .post(format!("{}/mcp", w.root))
            .bearer_auth(&w.alex)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":w.ws,"address":address}}}))
            .send()
            .await?;
        ensure!(response.status().is_success(), "{}", response.text().await?);
        let reply: Value = response.json().await?;
        let object: Value = serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        ensure!(
            object["annotations"]["comments"]["n1"] == *comment,
            "{reply}"
        );
    }
    Ok(())
}
