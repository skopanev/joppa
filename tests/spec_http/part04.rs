// Part 4 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_previous_assignee_cannot_complete_returned_work() -> Result<()> {
    // old-agent-locked-out-ac1
    // P is alex and Q is maria, two different human members.
    let w = world().await?;
    let (p, q) = (w.alex.as_str(), w.maria.as_str());
    w.command_as(p, json!({"op":"assess","req":"requirement:key","revision":2,"ac":"ac:key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"returned","revision":0,"parent":"ac:key-rights","data":{"title":"Complete the defined work","owner":"owner","body":"Implement the key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Create work for transfer"})).await?;
    w.command_as(
        p,
        json!({"op":"claim_task","id":"task:returned","revision":1}),
    )
    .await?;
    w.command_as(p, json!({"op":"release_task","id":"task:returned","revision":2,"reason":"Return the unfinished work"})).await?;
    w.command_as(
        q,
        json!({"op":"claim_task","id":"task:returned","revision":3}),
    )
    .await?;
    let claimed = w.get("address=task:returned").await?;
    ensure!(claimed["current_revision"] == 4, "{claimed}");
    ensure!(claimed["process"]["assignee"] == "maria", "{claimed}");
    let conn = w.pool.get().await?;
    let before: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    let request_id = uuid::Uuid::new_v4().to_string();
    let complete = json!({"op":"complete_task","commit":"c0ffee0","id":"task:returned","revision":4,"evidence":"Implemented the contract and recorded checks"});
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(p)
        .json(&json!({"request_id":request_id,"command":complete}))
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::CONFLICT,
        "{}",
        response.status()
    );
    let response: Value = response.json().await?;
    ensure!(
        response["error"] == "Task is not claimed unfinished work",
        "{response}"
    );
    let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(p).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":request_id,"command":complete}}})).send().await?;
    ensure!(response.status().is_success(), "{}", response.status());
    let response: Value = response.json().await?;
    ensure!(
        response["result"]["isError"] == true
            && response["result"]["content"][0]["text"] == "Task is not claimed unfinished work",
        "{response}"
    );
    let after: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(before == after, "The refused completion appended an event");
    let receipts: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2",
            &[&w.ws, &request_id],
        )
        .await?
        .get(0);
    ensure!(
        receipts == 0,
        "The refused completion consumed its request ID"
    );
    let unchanged = w.get("address=task:returned").await?;
    ensure!(
        unchanged["document"] == claimed["document"] && unchanged["process"] == claimed["process"],
        "The refused completion changed the task: {unchanged}"
    );
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(q)
        .json(&json!({"request_id":request_id,"command":complete}))
        .send()
        .await?;
    ensure!(response.status().is_success(), "{}", response.text().await?);
    let done = w.get("address=task:returned").await?;
    ensure!(
        done["current_revision"] == 5
            && done["document"]["actor"] == "maria"
            && done["process"]["done"].is_string(),
        "{done}"
    );
    w.command(json!({"op":"save_object","kind":"task","id":"owner-completion","revision":0,"parent":"ac:key-rights","data":{"title":"Check completion authority","owner":"owner","body":"Implement the same key rights contract","purpose":"implementation","projects":[]},"reason":"Check completion by the current holder"})).await?;
    w.command_as(
        q,
        json!({"op":"claim_task","id":"task:owner-completion","revision":1}),
    )
    .await?;
    let owned = w.get("address=task:owner-completion").await?;
    let complete = json!({"op":"complete_task","commit":"c0ffee0","id":"task:owner-completion","revision":2,"evidence":"Record the completed work"});
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":complete}))
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::CONFLICT,
        "{}",
        response.status()
    );
    let response: Value = response.json().await?;
    ensure!(
        response["error"] == "Task is not claimed unfinished work",
        "{response}"
    );
    let unchanged = w.get("address=task:owner-completion").await?;
    ensure!(
        unchanged["document"] == owned["document"] && unchanged["process"] == owned["process"],
        "The refused owner completion changed the Task"
    );
    w.command_as(q, complete).await?;
    let done = w.get("address=task:owner-completion").await?;
    ensure!(
        done["document"]["actor"] == "maria" && done["process"]["done"].is_string(),
        "{done}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_released_holder_cannot_complete_open_or_reclaimed_work() -> Result<()> {
    // old-agent-locked-out-ac1
    // P is alex and Q is maria. In one fixture, P also owns the Task document.
    for (p_owns_task, owner_releases) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        released_holder_cannot_complete(p_owns_task, owner_releases)
            .await
            .map_err(|e| {
                e.context(format!(
                    "P owns the Task: {p_owns_task}, owner releases: {owner_releases}"
                ))
            })?;
    }
    Ok(())
}

/// P (alex) claims T, then P or the workspace owner releases it; P's completion is refused
/// while T is open and after Q (maria) claims it, then Q completes T.
async fn released_holder_cannot_complete(p_owns_task: bool, owner_releases: bool) -> Result<()> {
    let w = world().await?;
    let (p, q) = (w.alex.as_str(), w.maria.as_str());
    let document_owner = if p_owns_task { "alex" } else { "owner" };
    w.command_as(p, json!({"op":"assess","req":"requirement:key","revision":2,"ac":"ac:key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"released","revision":0,"parent":"ac:key-rights","data":{"title":"Complete the defined work","owner":document_owner,"body":"Implement the key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Create work for release"})).await?;
    w.command_as(
        p,
        json!({"op":"claim_task","id":"task:released","revision":1}),
    )
    .await?;
    let claimed = w.get("address=task:released").await?;
    ensure!(
        claimed["process"]["assignee"] == "alex"
            && claimed["document"]["data"]["owner"] == document_owner,
        "{claimed}"
    );
    let release_token = if owner_releases { &w.owner } else { p };
    w.command_as(release_token, json!({"op":"release_task","id":"task:released","revision":2,"reason":"Return the unfinished work"})).await?;
    for mcp in [false, true] {
        let open = refused_completion(&w, p, "task:released", mcp).await?;
        ensure!(
            open["process"]["status"] == "open"
                && open["process"]["assignee"].is_null()
                && open["process"]["done"].is_null(),
            "{open}"
        );
    }
    w.command_as(
        q,
        json!({"op":"claim_task","id":"task:released","revision":3}),
    )
    .await?;
    for mcp in [false, true] {
        let claimed = refused_completion(&w, p, "task:released", mcp).await?;
        ensure!(
            claimed["process"]["status"] == "in_progress"
                && claimed["process"]["assignee"] == "maria"
                && claimed["process"]["done"].is_null(),
            "{claimed}"
        );
    }
    w.command_as(q, json!({"op":"complete_task","commit":"c0ffee0","id":"task:released","revision":4,"evidence":"Implemented the contract and recorded checks"})).await?;
    let done = w.get("address=task:released").await?;
    ensure!(
        done["current_revision"] == 5
            && done["document"]["actor"] == "maria"
            && done["process"]["done"].is_string(),
        "{done}"
    );
    Ok(())
}
