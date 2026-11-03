// Part 33 of tests/spec_http.rs, included there, so the test names stay unchanged.

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
    types["risk"] = json!({"title":"Risk","role":"document","status":"none","root":false,"parents":["capability"]});
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
    risk["risk"] = json!({"title":"Risk","role":"document","status":"none","root":false,"parents":["capability"]});
    job["job"] = json!({"title":"Jobs","role":"task","status":"none","root":true,"parents":["*"]});
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
