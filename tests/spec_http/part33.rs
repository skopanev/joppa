// Part 33 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_releases_an_absent_planners_claim() -> Result<()> {
    // owner-releases-claim-ac2
    for mcp in [false, true] {
        let w = world().await?;
        let p = store::create_agent_token(&w.pool, &w.ws, "owner", "P", None).await?;
        let p = p["token"].as_str().unwrap();
        let worker = store::create_agent_token(&w.pool, &w.ws, "owner", "W", None).await?;
        let worker = worker["token"].as_str().unwrap();
        w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
        let t0 = ready_task(&w, "t0", "key-rights").await?;
        w.command_as(worker, json!({"op":"claim_task","id":t0,"revision":1}))
            .await?;
        let revision = async |address: &str| -> Result<Value> {
            Ok(w.get(&format!("address={address}")).await?["current_revision"].clone())
        };
        w.command_as(p, json!({"op":"claim_planning","ac":"ac:key-rights","revision":revision("ac:key-rights").await?})).await?;
        for id in ["t1", "t2"] {
            w.command_as(p, ready_task_command(id, "key-rights"))
                .await?;
        }
        w.command_as(p, json!({"op":"save_object","kind":"task","id":"t0","revision":revision(&t0).await?,"parent":"ac:key-rights","data":{"title":"Implement the planned work","owner":"owner","contract":"Implement the criterion and record the result","purpose":"implementation","projects":[]},"reason":"Update the older work"})).await?;
        let snapshot = w.get("address=task:t0").await?;
        let history = object_history(&w, "task:t0").await?;
        ensure!(
            snapshot["document"]["actor"] == "P"
                && snapshot["process"]["assignee"] == "W"
                && history.as_array().unwrap().len() as u64
                    == snapshot["current_revision"].as_u64().unwrap(),
            "{snapshot} / {history}"
        );
        let release =
            |reason: &str| json!({"op":"release_planning","ac":"ac:key-rights","reason":reason});

        refused_without_change(
            &w,
            mcp,
            &w.bot,
            release("Planner unavailable"),
            "workspace owner required",
        )
        .await?;
        let claim = w.get("address=ac:key-rights").await?;
        ensure!(
            claim["process"]["planning_claim"]["actor"] == "P",
            "{claim}"
        );
        refused_without_change(&w, mcp, &w.owner, release(" "), "text must not be empty").await?;

        let reply = send_over(&w, mcp, &w.owner, &release("Planner unavailable")).await?;
        ensure!(reply.is_ok(), "mcp={mcp}: {reply:?}");
        let ac = w.get("address=ac:key-rights").await?;
        ensure!(ac["process"]["planning_claim"].is_null(), "{ac}");
        for id in ["task:t1", "task:t2"] {
            let retired = w.get(&format!("address={id}")).await?;
            ensure!(
                retired["process"]["status"] == "retired"
                    && retired["process"]["retired"] == "Planner unavailable"
                    && retired["document"]["present"] == true,
                "{retired}"
            );
        }
        let t0_after = w.get("address=task:t0").await?;
        ensure!(
            t0_after["process"]["status"] == "in_progress"
                && t0_after["process"]["assignee"] == "W"
                && t0_after["document"] == snapshot["document"]
                && t0_after["current_revision"] == snapshot["current_revision"]
                && object_history(&w, "task:t0").await? == history,
            "{snapshot} / {t0_after}"
        );
        let (actor, command) = last_event(&w).await?;
        ensure!(
            actor == "owner"
                && command["op"] == "release_planning"
                && command["reason"] == "Planner unavailable",
            "{actor}: {command}"
        );
        refused_without_change(
            &w,
            mcp,
            &w.owner,
            release("Planner unavailable"),
            "criterion has no planning claim",
        )
        .await?;
    }
    Ok(())
}

// tree-schema-revision-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_owner_schema_save_over_http_adds_one_revision_and_keeps_the_old_one() -> Result<()> {
    async fn tree(w: &World, query: &str) -> Result<Value> {
        let reply = w
            .http
            .get(format!("{}/api/workspaces/{}/tree{query}", w.root, w.ws))
            .bearer_auth(&w.owner)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status == 200, "GET /tree{query}: {status}: {body}");
        Ok(serde_json::from_str(&body)?)
    }
    let w = world().await?;
    let before = tree(&w, "").await?;
    let n = before["current_revision"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("current_revision needs a number: {before}"))?;
    let old_types = before["schema"]["types"].clone();
    ensure!(
        old_types.is_object(),
        "schema.types needs an object: {before}"
    );
    ensure!(
        old_types.get("risk").is_none(),
        "risk already exists: {before}"
    );
    let mut types = old_types.clone();
    types["risk"] = json!({"title":"Risk","role":"document","root":false,"parents":["capability"]});
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"set_tree_schema","revision":n,"types":types,"reason":"Add risk"}}))
        .send()
        .await?;
    let status = reply.status();
    let body = reply.text().await?;
    ensure!(status == 200, "set_tree_schema: {status}: {body}");
    let current = tree(&w, "").await?;
    let old = tree(&w, &format!("?revision={n}")).await?;
    println!("current: {current}\nrevision {n}: {old}");
    ensure!(
        current["current_revision"] == n + 1,
        "current_revision is not {}: {current}\nrevision {n}: {old}",
        n + 1
    );
    ensure!(
        current["schema"]["types"]["risk"]["title"] == "Risk",
        "the new type is missing: {current}\nrevision {n}: {old}"
    );
    ensure!(
        old["schema"]["revision"] == n,
        "schema.revision is not {n}: {current}\nrevision {n}: {old}"
    );
    ensure!(
        old["schema"]["types"] == old_types,
        "revision {n} lost its old types: {current}\nrevision {n}: {old}"
    );
    Ok(())
}

// tree-schema-revision-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_owner_schema_save_on_a_stale_revision_over_http_is_refused_and_changes_nothing()
-> Result<()> {
    async fn tree(w: &World) -> Result<Value> {
        let reply = w
            .http
            .get(format!("{}/api/workspaces/{}/tree", w.root, w.ws))
            .bearer_auth(&w.owner)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status == 200, "GET /tree: {status}: {body}");
        Ok(serde_json::from_str(&body)?)
    }
    let w = world().await?;
    let before = tree(&w).await?;
    let n = before["current_revision"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("current_revision needs a number: {before}"))?;
    let mut risk = before["schema"]["types"].clone();
    ensure!(risk.get("risk").is_none(), "risk already exists: {before}");
    let mut job = risk.clone();
    risk["risk"] = json!({"title":"Risk","role":"document","root":false,"parents":["capability"]});
    job["job"] = json!({"title":"Jobs","role":"task","root":true,"parents":["*"]});
    w.command(json!({"op":"set_tree_schema","revision":n,"types":risk,"reason":"Add risk"}))
        .await?;
    let control = tree(&w).await?;
    ensure!(
        control["current_revision"] == n + 1,
        "current_revision is not {}: {control}",
        n + 1
    );
    let body = w
        .refused(json!({"op":"set_tree_schema","revision":n,"types":job,"reason":"Add jobs"}))
        .await?;
    let reply: Value = serde_json::from_str(&body)?;
    ensure!(
        reply["error"] == "stale schema revision; read the tree schema again",
        "{body}"
    );
    let after = tree(&w).await?;
    ensure!(
        after["current_revision"] == n + 1,
        "current_revision is not {}: {after}",
        n + 1
    );
    ensure!(
        after == control,
        "a stale save changed the tree:\n{after}\ncontrol:\n{control}"
    );
    Ok(())
}
