// Part 17 of tests/spec_http.rs, included there, so the test names stay unchanged.

impl World {
    /// The error text of a command that must be refused.
    async fn refused(&self, command: Value) -> Result<String> {
        let reply = self
            .http
            .post(format!("{}/api/workspaces/{}/commands", self.root, self.ws))
            .bearer_auth(&self.owner)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
            .send()
            .await?;
        ensure!(!reply.status().is_success(), "{command} must be refused");
        Ok(reply.text().await?)
    }
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_edit_adds_a_revision_and_keeps_the_original() -> Result<()> {
    // edits-keep-original-ac1
    let w = world().await?;
    w.note("a", 0, "A", "original text", json!([])).await?;
    w.note("a", 1, "A", "edited text", json!([])).await?;
    let current = w.get("address=note:a").await?;
    let first = w.get("address=note:a&revision=1").await?;
    ensure!(
        current["document"]["revision"] == 2
            && current["document"]["data"]["body"] == "edited text",
        "{current}"
    );
    ensure!(
        first["document"]["data"]["body"] == "original text",
        "{first}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_edit_on_a_stale_revision_is_refused() -> Result<()> {
    // edits-keep-original-ac2
    let w = world().await?;
    w.note("a", 0, "A", "original text", json!([])).await?;
    w.note("a", 1, "A", "edited text", json!([])).await?;
    let error = w
        .refused(json!({"op":"save_object","kind":"note","id":"a","revision":1,"parent":null,"data":{"title":"A","owner":"owner","body":"late text"},"links":[],"reason":"Save"}))
        .await?;
    ensure!(
        error.contains("stale object revision; read the object again"),
        "{error}"
    );
    let current = w.get("address=note:a").await?;
    ensure!(
        current["document"]["data"]["body"] == "edited text",
        "{current}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_link_to_another_workspace_is_refused() -> Result<()> {
    // workspaces-isolated-ac2
    let a = world().await?;
    let b = world().await?;
    b.note("only-b", 0, "B", "lives in B", json!([])).await?;
    a.note("a", 0, "A", "lives in A", json!([])).await?;
    let error = a
        .refused(json!({"op":"save_object","kind":"note","id":"a","revision":1,"parent":null,"data":{"title":"A","owner":"owner","body":"lives in A"},"links":[{"target":"note:only-b","relation":"related"}],"reason":"Link"}))
        .await?;
    ensure!(
        error.contains("unknown linked object in this workspace"),
        "{error}"
    );

    // links-in-workspace-ac1: the bot holds a write grant for workspace A only.
    let grant = store::create_agent_token(&a.pool, &a.ws, "owner", "bot", Some("write")).await?;
    let bot = grant["token"].as_str().unwrap();
    let stored = store::token_actor(&a.pool, bot).await?.unwrap();
    ensure!(
        stored.agent == "bot" && stored.workspace == a.ws && stored.access == store::Access::Write,
        "{stored:?}"
    );
    a.note("also-a", 0, "Also A", "lives in A", json!([]))
        .await?;
    let link = |revision: u32, target: &'static str| {
        let a = &a;
        async move {
            let reply = a
                .http
                .post(format!("{}/api/workspaces/{}/commands", a.root, a.ws))
                .bearer_auth(bot)
                .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"save_object","kind":"note","id":"a","revision":revision,"parent":null,"data":{"title":"A","owner":"owner","body":"lives in A"},"links":[{"target":target,"relation":"related"}],"reason":"Link"}}))
                .send()
                .await?;
            let status = reply.status();
            let text = reply.text().await?;
            Ok::<_, anyhow::Error>((status, serde_json::from_str(&text).unwrap_or(json!(text))))
        }
    };

    // The control: the same credential links two objects inside A.
    let (status, reply) = link(1, "note:also-a").await?;
    ensure!(status.is_success(), "control link: {status} {reply}");
    let before = saved_rows(&a).await?;
    ensure!(
        before["object_links"].as_array().is_some_and(|links| links
            .iter()
            .any(|l| l["source"] == "note:a"
                && l["target"] == "note:also-a"
                && l["relation"] == "related")),
        "{}",
        before["object_links"]
    );
    ensure!(
        before["objects"]
            .as_array()
            .is_some_and(|objects| objects.iter().all(|o| o["address"] != "note:only-b")),
        "note:only-b must exist only in workspace B: {}",
        before["objects"]
    );

    // The link to the address that exists only in B is refused and changes nothing in A.
    let (status, reply) = link(2, "note:only-b").await?;
    ensure!(
        !status.is_success(),
        "cross-workspace link: {status} {reply}"
    );
    ensure!(
        reply["error"] == "unknown linked object in this workspace",
        "{status} {reply}"
    );
    let after = saved_rows(&a).await?;
    for table in ["objects", "object_revisions", "object_links"] {
        ensure!(
            after[table] == before[table],
            "{table} changed after the refusal: {} -> {}",
            before[table],
            after[table]
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_inbox_lists_undecided_inputs_before_decided_inputs() -> Result<()> {
    // undecided-first-ac1
    let w = world().await?;
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let token = grant["token"].as_str().unwrap();
    for id in ["a-decided", "b-decided", "z-undecided"] {
        w.note(id, 0, id, "Input", json!([])).await?;
        if id != "z-undecided" {
            w.command(json!({"op":"decide_note","target":format!("note:{id}"),"revision":1,"outcome":"changed_spec","reason":"Input reviewed","links":["domain:security"]})).await?;
        }
    }
    let expected = ["note:z-undecided", "note:a-decided", "note:b-decided"];
    let http = w.get("kind=note&parent=*&limit=100").await?;
    ensure!(http["items"][0]["status"] == "unprocessed", "{http}");
    let mut cursor = None;
    for (i, address) in expected.into_iter().enumerate() {
        let mut arguments = json!({"workspace":w.ws,"kind":"note","parent":"*","limit":1});
        if let Some(cursor) = cursor {
            arguments["cursor"] = cursor;
        }
        let response: Value = w.http.post(format!("{}/mcp", w.root)).bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":(crate::legacy_read_args("joppa_objects", &arguments))}}))
            .send().await?.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        let page: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        ensure!(
            page["items"].as_array().unwrap().len() == 1 && page["items"][0]["address"] == address,
            "{page}"
        );
        ensure!(page["items"][0] == http["items"][i], "{page} / {http}");
        cursor = page["next_cursor"].as_str().map(|s| json!(s));
    }
    ensure!(cursor.is_none(), "the last page must finish the list");
    Ok(())
}
