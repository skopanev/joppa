// Part 29 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// Create one ready implementation Task under an AC of a REQ.
async fn ready_task(w: &World, id: &str, ac: &str) -> Result<String> {
    w.command(ready_task_command(id, ac)).await?;
    Ok(format!("task:{id}"))
}

fn ready_task_command(id: &str, ac: &str) -> Value {
    json!({"op":"save_object","kind":"task","id":id,"revision":0,"parent":format!("ac:{ac}"),"data":{"title":"Implement the specified work","owner":"owner","contract":"Implement the criterion and record the result","purpose":"implementation","projects":[]},"reason":"Plan the criterion"})
}

async fn journal_position(w: &World) -> Result<i64> {
    let conn = w.pool.get().await?;
    let row = conn
        .query_one(
            "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?;
    Ok(row.get(0))
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_identical_save_adds_no_revision_or_journal_event() -> Result<()> {
    // edit-makes-revision-ac3
    let w = world().await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let t = ready_task(&w, "t", "key-rights").await?;
    let save = |revision: u32, title: &str| json!({"op":"save_object","kind":"task","id":"t","revision":revision,"parent":"ac:key-rights","data":{"title":title,"owner":"owner","contract":"Implement the changed criterion and record the result","purpose":"implementation","projects":[]},"reason":"Save the work contract"});
    let title = "Implement the specified work";
    w.command(save(1, title)).await?;
    let read = async |address: &str| w.get(&format!("address={address}")).await;
    let revisions = async || w.get("address=task:t&history=true").await;
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":t,"revision":read(&t).await?["current_revision"]}),
    )
    .await?;
    w.command_as(&w.bot, json!({"op":"complete_task","id":t,"revision":read(&t).await?["current_revision"],"evidence":"Implemented the contract"})).await?;
    let done = read(&t).await?;
    ensure!(
        done["current_revision"] == 4
            && done["process"]["status"] == "done"
            && done["process"]["assignee"] == "bot",
        "{done}"
    );
    w.command(json!({"op":"save_object","kind":"task","id":"u","revision":0,"parent":"ac:key-rights","data":{"title":"Use the specified work","owner":"owner","contract":"Use the completed work and record the result","purpose":"implementation","projects":[]},"links":[{"target":t,"relation":"depends_on","revision":4,"blocking":true}],"reason":"Depend on the completed work"})).await?;
    let position = journal_position(&w).await?;

    let http = w.command_as(&w.bot, save(4, title)).await?;
    ensure!(
        http["object"] == json!({"address":t,"revision":4}),
        "{http}"
    );
    let mcp = mcp_value(&w, "joppa_command", json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":save(4, title)})).await?;
    ensure!(mcp["object"] == json!({"address":t,"revision":4}), "{mcp}");
    let task = revisions().await?;
    ensure!(
        task["current_revision"] == 4 && task["revision_count"] == 4,
        "{task}"
    );
    let u = read("task:u").await?;
    ensure!(
        u["review_required"] == false
            && u["document"]["links"][0]["revision"] == task["current_revision"],
        "{u}"
    );
    let available = mcp_value(
        &w,
        "joppa_tasks",
        json!({"workspace":w.ws,"view":"available"}),
    )
    .await?;
    ensure!(
        available["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["address"] == "task:u" && row["claimable"] == true),
        "{available}"
    );
    ensure!(journal_position(&w).await? == position);

    for (token, command, error) in [
        (
            &w.bot,
            save(4, "Implement other work"),
            "completed or retired Task cannot be amended",
        ),
        (
            &w.bot,
            save(3, title),
            "stale object revision; read the object again",
        ),
        (
            &w.maria,
            save(4, title),
            "Task owner, assignee or unclaimed creator required",
        ),
    ] {
        let reply = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(token)
            .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}))
            .send()
            .await?;
        ensure!(!reply.status().is_success(), "{command}");
        let reply: Value = reply.json().await?;
        ensure!(reply["error"] == error, "{command}: {reply}");
    }
    let task = revisions().await?;
    ensure!(
        task["current_revision"] == 4 && task["revision_count"] == 4,
        "{task}"
    );
    ensure!(journal_position(&w).await? == position);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_claims_and_completes_all_tasks_of_a_req_in_one_batch() -> Result<()> {
    // one-task-per-agent-ac3
    let w = world().await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let mut tasks = vec![];
    for id in ["t1", "t2", "t3"] {
        tasks.push(ready_task(&w, id, "key-rights").await?);
    }
    let read = async |address: &str| w.get(&format!("address={address}")).await;
    let claims: Vec<Value> = tasks
        .iter()
        .map(|id| json!({"command":{"op":"claim_task","id":id,"revision":1}}))
        .collect();
    mcp_value(
        &w,
        "joppa_batch",
        json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"operations":claims}),
    )
    .await?;
    let mut completions = vec![];
    for id in &tasks {
        let task = read(id).await?;
        ensure!(
            task["process"]["assignee"] == "bot" && task["process"]["status"] == "in_progress",
            "{task}"
        );
        completions.push(json!({"command":{"op":"complete_task","id":id,"revision":task["current_revision"],"evidence":"Implemented the criterion and ran its checks"}}));
    }
    mcp_value(&w, "joppa_batch", json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"operations":completions})).await?;
    let mut done = vec![];
    for id in &tasks {
        let task = read(id).await?;
        ensure!(
            task["process"]["assignee"] == "bot" && task["process"]["status"] == "done",
            "{task}"
        );
        done.push(task);
    }
    w.pool
        .get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&w.ws])
        .await?;
    store::migrate(&w.pool).await?;
    for (id, before) in tasks.iter().zip(&done) {
        let after = read(id).await?;
        ensure!(
            after["process"] == before["process"],
            "journal replay changed {id}: {before} / {after}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_batch_claim_with_a_held_task_changes_nothing() -> Result<()> {
    // one-task-per-agent-ac4
    let w = world().await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let mut tasks = vec![];
    for id in ["t1", "t2", "t3"] {
        tasks.push(ready_task(&w, id, "key-rights").await?);
    }
    let other = store::create_agent_token(&w.pool, &w.ws, "owner", "other", None).await?;
    let other = other["token"].as_str().unwrap();
    w.command_as(other, json!({"op":"claim_task","id":tasks[2],"revision":1}))
        .await?;
    let read = async |address: &str| w.get(&format!("address={address}")).await;
    let mut claims = vec![];
    for id in &tasks {
        let task = read(id).await?;
        claims.push(
            json!({"command":{"op":"claim_task","id":id,"revision":task["current_revision"]}}),
        );
    }
    let position = journal_position(&w).await?;
    let before = saved_rows(&w).await?;

    let batch = mcp_call(
        &w,
        &w.bot,
        "joppa_batch",
        json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"operations":claims}),
    )
    .await?;
    ensure!(batch["isError"] == true, "{batch}");
    for id in &tasks[..2] {
        let task = read(id).await?;
        ensure!(task["process"]["assignee"].is_null(), "{task}");
    }
    let held = read(&tasks[2]).await?;
    ensure!(held["process"]["assignee"] == "other", "{held}");
    ensure!(journal_position(&w).await? == position);
    ensure!(
        saved_rows(&w).await? == before,
        "a refused batch changed state"
    );

    let t1 = read(&tasks[0]).await?;
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":tasks[0],"revision":t1["current_revision"]}),
    )
    .await?;
    let t1 = read(&tasks[0]).await?;
    ensure!(t1["process"]["assignee"] == "bot", "{t1}");
    Ok(())
}
