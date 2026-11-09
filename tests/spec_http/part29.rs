// Part 29 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// Create one ready implementation Task under an AC of a REQ.
async fn ready_task(w: &World, id: &str, ac: &str) -> Result<String> {
    w.command(ready_task_command(id, ac)).await?;
    Ok(format!("task:{id}"))
}

fn ready_task_command(id: &str, ac: &str) -> Value {
    json!({"op":"save_object","kind":"task","id":id,"revision":0,"parent":format!("ac:{ac}"),"data":{"title":"Implement the specified work","owner":"owner","body":"Implement the criterion and record the result","projects":[]},"reason":"Plan the criterion"})
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
    let t = ready_task(&w, "t", "key-rights").await?;
    let title = "Implement the specified work";
    let save = |revision: u32, title: &str, extra: Value| {
        let mut data = json!({"title":title,"owner":"owner","body":"Implement the changed criterion and record the result","projects":[]});
        for (key, value) in extra.as_object().into_iter().flatten() {
            data[key] = value.clone();
        }
        json!({"op":"save_object","kind":"task","id":"t","revision":revision,"parent":"ac:key-rights","data":data,"reason":"Save the work contract"})
    };
    let done = json!({"assignee":"bot","commit":"c0ffee0"});
    w.command(save(1, title, json!({}))).await?;
    let read = async |address: &str| w.get(&format!("address={address}")).await;
    let revisions = async || w.get("address=task:t&history=true").await;
    w.command_as(&w.bot, save(2, title, json!({"assignee":"bot"})))
        .await?;
    w.command_as(&w.bot, save(3, title, done.clone())).await?;
    let finished = read(&t).await?;
    ensure!(
        finished["current_revision"] == 4 && finished["document"]["data"]["assignee"] == "bot",
        "{finished}"
    );
    w.command(json!({"op":"save_object","kind":"task","id":"u","revision":0,"parent":"ac:key-rights","data":{"title":"Use the specified work","owner":"owner","body":"Use the completed work and record the result","projects":[]},"relations":{"depends_on":{"edges":[{"to":t,"revision":4}]}},"reason":"Depend on the completed work"})).await?;
    let position = journal_position(&w).await?;

    // The write gives the current revision of the parent too.
    let parent = read("ac:key-rights").await?["current_revision"].clone();
    ensure!(parent.is_u64(), "{parent}");
    let object = json!({"address":t,"revision":4,"parent_revision":parent});
    let http = w.command_as(&w.bot, save(4, title, done.clone())).await?;
    ensure!(http["object"] == object, "{http}");
    let mcp = mcp_value(&w, "joppa_command", json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":save(4, title, done.clone())})).await?;
    ensure!(mcp["object"] == object, "{mcp}");
    let task = revisions().await?;
    ensure!(
        task["current_revision"] == 4 && task["revisions"].as_array().map(Vec::len) == Some(4),
        "{task}"
    );
    let u = read("task:u").await?;
    ensure!(
        u.get("review_required").is_none()
            && u["document"]["relations"]["depends_on"]["edges"][0]["revision"]
                == task["current_revision"],
        "{u}"
    );
    // joppa_objects over MCP reads the same revisions and the same pin.
    let mcp_task = mcp_value(
        &w,
        "joppa_objects",
        json!({"workspace":w.ws,"address":t,"history":true}),
    )
    .await?;
    ensure!(
        mcp_task["current_revision"] == 4
            && mcp_task["items"].as_array().map(Vec::len) == Some(4)
            && mcp_task["total"] == 4,
        "{mcp_task}"
    );
    let mcp_u = mcp_value(
        &w,
        "joppa_objects",
        json!({"workspace":w.ws,"address":"task:u"}),
    )
    .await?;
    ensure!(
        mcp_u["relations"]["depends_on"]["edges"][0]["revision"] == 4,
        "{mcp_u}"
    );
    ensure!(journal_position(&w).await? == position);

    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.bot)
        .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":save(3, title, done.clone())}))
        .send()
        .await?;
    ensure!(!reply.status().is_success(), "a stale save is refused");
    let reply: Value = reply.json().await?;
    ensure!(
        reply["error"] == "stale object revision; read the object again",
        "{reply}"
    );
    let task = revisions().await?;
    ensure!(
        task["current_revision"] == 4 && task["revisions"].as_array().map(Vec::len) == Some(4),
        "{task}"
    );
    ensure!(journal_position(&w).await? == position);
    // Any member changes the finished Task: it gets one revision and keeps its data.
    w.command_as(&w.maria, save(4, "Implement other work", done))
        .await?;
    let amended = read(&t).await?;
    ensure!(
        amended["current_revision"] == 5 && amended["document"]["data"]["commit"] == "c0ffee0",
        "{amended}"
    );
    Ok(())
}
