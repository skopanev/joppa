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
    // Two tests prove the criterion; the list is saved before the readiness assessment.
    let tests = ["tests/key.rs > first_boundary", "tests/key.rs > second_boundary"];
    w.command(json!({"op":"set_ac_tests","target":"ac:key-rights","tests":tests,"reason":"Bind"})).await?;
    let a = object_as(&w, &w.bot, "ac:key-rights", None).await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"requirement:key","revision":a["process"]["assessment"]["revision"],"ac":"ac:key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
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
    // P records a partial result: the first test passes at def4567, the second has no result.
    let partial = json!({"workspace":w.ws,"commit":"def4567","results":[{"test":tests[0],"status":"pass"}]});
    let recorded = mcp_call(&w, &w.bot, "joppa_record_results", partial).await?;
    ensure!(recorded["isError"] != true, "{recorded}");

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
        available["items"]
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
    // MCP gives the comments as one page; the author is the actor when they agree.
    let note = |object: &Value, id: &str| {
        object["comments"]["items"]
            .as_array()
            .and_then(|items| items.iter().find(|c| c["id"] == id))
            .cloned()
            .unwrap_or_default()
    };
    for (id, text) in notes {
        let (old, new) = (note(&before, id), note(&task, id));
        ensure!(new == old, "note {id} changed: {old} became {new}");
        ensure!(
            old["actor"] == "bot" && old["text"] == text,
            "note {id} lost its author or text: {old}"
        );
    }
    ensure!(
        task["comments"] == before["comments"],
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

    // Q reads the partial result that P recorded.
    let status = mcp_call(&w, bot2, "joppa_tests", json!({"workspace":w.ws,"ac":"ac:key-rights"})).await?;
    let status: Value = serde_json::from_str(status["content"][0]["text"].as_str().unwrap_or("null"))?;
    let first = status["tests"].as_array().and_then(|t| t.iter().find(|t| t["address"] == tests[0]).cloned());
    ensure!(
        first.is_some_and(|t| t["status"] == "pass" && t["commit"] == "def4567") && status["status"] != "green",
        "{status}"
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
        // MCP gives the same comment in its page, without the repeated author and created_at.
        let item = object["comments"]["items"]
            .as_array()
            .and_then(|items| items.iter().find(|c| c["id"] == "n1"))
            .cloned()
            .unwrap_or_default();
        ensure!(
            item["actor"] == comment["author"]
                && item["actor_type"] == "person"
                && item["at"] == comment["created_at"]
                && item["text"] == comment["text"]
                && item.get("author").is_none(),
            "{reply}"
        );
    }
    Ok(())
}
