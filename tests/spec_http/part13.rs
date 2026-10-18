// Part 13 of tests/spec_http.rs, included there, so the test names stay unchanged.

// [ac:id-survives-move-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn rename_and_move_keep_the_address_and_the_item_id() -> Result<()> {
    let w = world().await?;
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", Some("write")).await?;
    let bot = grant["token"].as_str().unwrap();
    let read = || async {
        let reply = w
            .http
            .get(format!(
                "{}/api/workspaces/{}/objects?address=requirement:key",
                w.root, w.ws
            ))
            .bearer_auth(bot)
            .send()
            .await?;
        ensure!(reply.status() == 200, "read: {}", reply.text().await?);
        Ok::<Value, anyhow::Error>(reply.json().await?)
    };
    let before = read().await?;
    ensure!(
        before["item_id"].as_str().is_some_and(|id| !id.is_empty()),
        "{before}"
    );
    ensure!(
        before["document"]["parent"] != "capability:empty",
        "{before}"
    );
    let mut data = before["document"]["data"].clone();
    data["title"] = json!("Key rights, renamed");
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(bot)
        .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"save_object","kind":"requirement","id":"key","revision":before["document"]["revision"],"parent":"capability:empty","data":data,"links":[],"reason":"Rename and move"}}))
        .send()
        .await?;
    ensure!(reply.status() == 200, "write: {}", reply.text().await?);
    let after = read().await?;
    ensure!(
        after["address"] == before["address"]
            && after["item_id"] == before["item_id"]
            && after["document"]["parent"] == "capability:empty"
            && after["document"]["data"]["title"] == "Key rights, renamed",
        "{before} -> {after}"
    );
    Ok(())
}

// [ac:id-survives-move-ac2]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_move_under_a_domain_is_refused_and_changes_nothing() -> Result<()> {
    let w = world().await?;
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", Some("write")).await?;
    let bot = grant["token"].as_str().unwrap();
    let read = || async {
        let reply = w
            .http
            .get(format!(
                "{}/api/workspaces/{}/objects?address=requirement:key",
                w.root, w.ws
            ))
            .bearer_auth(bot)
            .send()
            .await?;
        ensure!(reply.status() == 200, "read: {}", reply.text().await?);
        Ok::<Value, anyhow::Error>(reply.json().await?)
    };
    let move_to = |current: &Value, parent: &str| json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"save_object","kind":"requirement","id":"key","revision":current["document"]["revision"],"parent":parent,"data":current["document"]["data"],"links":[],"reason":"Move"}});
    let commands = format!("{}/api/workspaces/{}/commands", w.root, w.ws);
    let start = read().await?;
    let reply = w
        .http
        .post(&commands)
        .bearer_auth(bot)
        .json(&move_to(&start, "capability:empty"))
        .send()
        .await?;
    let status = reply.status();
    ensure!(
        status == 200,
        "control move: {status} {}",
        reply.text().await?
    );
    let before = read().await?;
    ensure!(
        before["item_id"].as_str().is_some_and(|id| !id.is_empty())
            && before["document"]["parent"] == "capability:empty",
        "{before}"
    );
    let reply = w
        .http
        .post(&commands)
        .bearer_auth(bot)
        .json(&move_to(&before, "domain:security"))
        .send()
        .await?;
    let status = reply.status();
    let body: Value = reply.json().await?;
    ensure!(
        status == 409 && body["error"] == "parent type is not permitted by the tree schema",
        "refused move: {status} {body}"
    );
    let after = read().await?;
    ensure!(
        after["address"] == before["address"]
            && after["item_id"] == before["item_id"]
            && after["document"]["parent"] == before["document"]["parent"]
            && after["document"]["revision"] == before["document"]["revision"],
        "{before} -> {after}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_stored_revision_on_an_ordering_link_asks_for_no_review() -> Result<()> {
    // blocking-pins-revision-ac1, narrowed by dependencies-before-priority: only a Task
    // dependency pins its target. A note keeps the stored revision, and it gates nothing.
    let w = world().await?;
    w.note("target", 0, "Target", "v1", json!([])).await?;
    w.note(
        "source",
        0,
        "Source",
        "s",
        json!([{"target":"note:target","relation":"depends_on","revision":1,"blocking":true}]),
    )
    .await?;
    w.note("target", 1, "Target", "v2", json!([])).await?;
    let source = w.get("address=note:source").await?;
    ensure!(
        source["document"]["links"][0]["revision"] == 1
            && source["document"]["links"][0]["blocking"] == true
            && source["review_required"] == false,
        "{source}"
    );
    Ok(())
}

// blocking-pins-revision-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot_adds_and_removes_a_blocking_link_that_stores_no_target_revision() -> Result<()> {
    let w = world().await?;
    let save = |id: &str, revision: u32, body: &str, links: Value| json!({"op":"save_object","kind":"note","id":id,"revision":revision,"parent":null,"data":{"title":id,"owner":"owner","body":body},"links":links,"reason":"Save"});
    // T is at revision 3 before the link.
    for revision in 0..3 {
        w.command(save(
            "t",
            revision,
            &format!("T at revision {}", revision + 1),
            json!([]),
        ))
        .await?;
    }
    w.command(save("s", 0, "Source", json!([]))).await?;
    let t = w.get("address=note:t").await?;
    ensure!(t["current_revision"] == 3, "{t}");
    // Agent bot adds a blocking link from S to T. The link names no revision.
    let link = json!([{"target":"note:t","relation":"depends_on","blocking":true}]);
    w.command_as(&w.bot, save("s", 1, "Source", link)).await?;
    // T moves to revision 4.
    w.command(save("t", 3, "T at revision 4", json!([])))
        .await?;
    let s = w.get("address=note:s").await?;
    let stored = &s["document"]["links"][0];
    let shown = &s["links"][0];
    ensure!(
        s["document"]["actor"] == "bot"
            && s["document"]["links"].as_array().map(Vec::len) == Some(1)
            && s["links"].as_array().map(Vec::len) == Some(1)
            && stored == &json!({"target":"note:t","relation":"depends_on","blocking":true})
            && shown["link"] == *stored
            && shown["blocking"] == true
            && shown.get("revision").is_none()
            && shown["current_revision"] == 4
            && s["review_required"] == false,
        "after revision 4 of T: {s}"
    );
    // Agent bot removes the link.
    w.command_as(&w.bot, save("s", 2, "Source", json!([])))
        .await?;
    let s = w.get("address=note:s").await?;
    ensure!(
        s["document"]["actor"] == "bot"
            && s["links"] == json!([])
            && s["document"]["links"] == json!([]),
        "after the removal: {s}"
    );
    Ok(())
}

async fn journal_refuses(sql: &str) -> Result<()> {
    // As an application role that has UPDATE and DELETE rights, so only the WORM
    // trigger can refuse; a superuser could drop the trigger and prove nothing.
    let w = world().await?;
    let mut conn = w.pool.get().await?;
    conn.batch_execute(
        "DO $$ BEGIN
            IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'joppa_test_app') THEN
                CREATE ROLE joppa_test_app NOLOGIN NOSUPERUSER;
            END IF;
         END $$;
         GRANT USAGE ON SCHEMA joppa TO joppa_test_app;
         GRANT SELECT, UPDATE, DELETE ON joppa.events TO joppa_test_app;",
    )
    .await?;
    let tx = conn.transaction().await?;
    tx.batch_execute("SET LOCAL ROLE joppa_test_app").await?;
    let error = tx
        .execute(sql, &[&w.ws])
        .await
        .expect_err("the journal must refuse");
    let db = error.as_db_error().expect("a database error");
    ensure!(
        db.code().code() == "P0001"
            && db.message() == "WORM: append a correction; history cannot be changed",
        "{error}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_journal_refuses_update() -> Result<()> {
    // record-append-only-ac1
    journal_refuses("UPDATE joppa.events SET actor=actor WHERE workspace=$1").await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_journal_refuses_delete() -> Result<()> {
    // record-append-only-ac2
    journal_refuses("DELETE FROM joppa.events WHERE workspace=$1").await
}
