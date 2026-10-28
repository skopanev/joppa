// Part 11 of tests/spec_http.rs, included there, so the test names stay unchanged.

// tree-schema-safe-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn removing_a_type_with_three_objects_is_refused_over_http() -> Result<()> {
    // tree-schema-safe-ac1
    // skopanev owns the workspace; note:first, note:second and note:third exist.
    let mut w = world().await?;
    w.ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [json!({"op":"workspace","title":"Owned by skopanev"})]
        .into_iter()
        .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "skopanev", &format!("setup-{i}"), &op).await?;
    }
    let ids = ["first", "second", "third"];
    for id in ids {
        w.command_as(&w.skopanev, json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"skopanev","body":"Scope"},"links":[],"reason":"Save"})).await?;
    }
    let tree = async || -> Result<Value> {
        let reply = w
            .http
            .get(format!("{}/api/workspaces/{}/tree", w.root, w.ws))
            .bearer_auth(&w.skopanev)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status.is_success(), "GET /tree: {status} {body}");
        Ok(serde_json::from_str(&body)?)
    };
    let before = tree().await?;
    let n = before["current_revision"].clone();
    ensure!(n.is_u64(), "{before}");
    let mut types = before["schema"]["types"].clone();
    ensure!(
        types
            .as_object_mut()
            .and_then(|t| t.remove("note"))
            .is_some(),
        "{before}"
    );
    let remove = json!({"op":"set_tree_schema","revision":n,"types":types,"reason":"Remove notes"});
    for mcp in [false, true] {
        refused_without_change(
            &w,
            mcp,
            &w.skopanev,
            remove.clone(),
            "object type is absent from the tree schema",
        )
        .await?;
    }
    for id in ids {
        let note = w.get_as(&w.skopanev, &format!("address=note:{id}")).await?;
        ensure!(note["document"]["present"] == true, "{note}");
    }
    let after = tree().await?;
    ensure!(
        after["current_revision"] == n && after["schema"]["types"].get("note").is_some(),
        "{after}"
    );
    Ok(())
}

// tree-schema-safe-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_parent_rule_that_orphans_risk_objects_is_refused_over_http() -> Result<()> {
    // tree-schema-safe-ac2
    // skopanev owns the workspace; two risks sit under capability:restrict.
    let mut w = world().await?;
    w.ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Owned by skopanev"}),
        json!({"op":"consumer","id":"customer","title":"Customer","owner":"skopanev"}),
        json!({"op":"domain","id":"security","title":"Security","authority":"skopanev"}),
        json!({"op":"capability","id":"restrict","title":"Restrict access","domain":"security"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "skopanev", &format!("setup-{i}"), &op).await?;
    }
    let tree = async || -> Result<Value> {
        let reply = w
            .http
            .get(format!("{}/api/workspaces/{}/tree", w.root, w.ws))
            .bearer_auth(&w.skopanev)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status.is_success(), "GET /tree: {status} {body}");
        Ok(serde_json::from_str(&body)?)
    };
    let before = tree().await?;
    let n = before["current_revision"].as_u64();
    ensure!(n.is_some(), "{before}");
    let n = n.unwrap();
    let mut types = before["schema"]["types"].clone();
    ensure!(types.get("risk").is_none(), "{before}");
    types["risk"] = json!({"title":"Risk","role":"document","status":"none","root":false,"parents":["capability"],"references":{"consumers":{"target":"consumer","title":"Used by","multiple":true,"required":false,"verification_subject":false}}});
    let (status, reply) = text_rule_request(
        &w,
        &w.skopanev,
        &uuid::Uuid::new_v4().to_string(),
        json!({"op":"set_tree_schema","revision":n,"types":types,"reason":"Add risk"}),
        "http",
    )
    .await?;
    ensure!(
        (200..300).contains(&status) && reply.get("error").is_none(),
        "{status} {reply}"
    );
    for id in ["leak", "outage"] {
        let save = json!({"op":"save_object","kind":"risk","id":id,"revision":0,"parent":"capability:restrict","data":{"title":id,"owner":"skopanev","body":"Scope"},"reason":"Define"});
        let (ok, reply) = send_as(&w, &w.skopanev, &save).await?;
        ensure!(ok, "{reply}");
    }
    types["risk"]["parents"] = json!(["domain"]);
    let narrow =
        json!({"op":"set_tree_schema","revision":n + 1,"types":types,"reason":"Move risk"});
    for mcp in [false, true] {
        refused_without_change(
            &w,
            mcp,
            &w.skopanev,
            narrow.clone(),
            "parent type is not permitted by the tree schema",
        )
        .await?;
    }
    let after = tree().await?;
    ensure!(
        after["current_revision"] == n + 1
            && after["schema"]["types"]["risk"]["parents"] == json!(["capability"]),
        "{after}"
    );
    for id in ["leak", "outage"] {
        let risk = w.get_as(&w.skopanev, &format!("address=risk:{id}")).await?;
        ensure!(
            risk["document"]["present"] == true
                && risk["document"]["parent"] == "capability:restrict",
            "{risk}"
        );
    }
    Ok(())
}

// tree-schema-safe-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_badge_change_keeps_the_object_revision_and_document_over_http() -> Result<()> {
    // tree-schema-safe-ac3
    // skopanev owns the workspace; note:kept sits at revision 2.
    let mut w = world().await?;
    w.ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [json!({"op":"workspace","title":"Owned by skopanev"})]
        .into_iter()
        .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "skopanev", &format!("setup-{i}"), &op).await?;
    }
    for (revision, body) in [(0, "First"), (1, "Second")] {
        w.command_as(&w.skopanev, json!({"op":"save_object","kind":"note","id":"kept","revision":revision,"parent":null,"data":{"title":"Kept","owner":"skopanev","body":body},"links":[],"reason":"Save"})).await?;
    }
    let hash = |read: &Value| -> Result<String> {
        let document = serde_json::to_string(&read["document"])?;
        Ok(joppa::digest_hex::sha256_hex(document.as_bytes()))
    };
    let http = w.get_as(&w.skopanev, "address=note:kept").await?;
    let mcp = object_as(&w, &w.skopanev, "note:kept", None).await?;
    let r = http["current_revision"].clone();
    ensure!(r == 2 && mcp["current_revision"] == r, "{http} {mcp}");
    let (http_hash, mcp_hash) = (hash(&http)?, hash(&mcp)?);
    let history = object_history(&w, "note:kept").await?;
    let tree = async || -> Result<Value> {
        let reply = w
            .http
            .get(format!("{}/api/workspaces/{}/tree", w.root, w.ws))
            .bearer_auth(&w.skopanev)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status.is_success(), "GET /tree: {status} {body}");
        Ok(serde_json::from_str(&body)?)
    };
    let before = tree().await?;
    let n = before["current_revision"].as_u64();
    ensure!(n.is_some(), "{before}");
    let n = n.unwrap();
    let mut types = before["schema"]["types"].clone();
    types["note"]["badge"] = json!("INPUT");
    let (status, reply) = text_rule_request(
        &w,
        &w.skopanev,
        &uuid::Uuid::new_v4().to_string(),
        json!({"op":"set_tree_schema","revision":n,"types":types,"reason":"Rename the note badge"}),
        "http",
    )
    .await?;
    ensure!(
        (200..300).contains(&status) && reply.get("error").is_none(),
        "{status} {reply}"
    );
    let after = tree().await?;
    ensure!(
        after["current_revision"] == n + 1 && after["schema"]["types"]["note"]["badge"] == "INPUT",
        "{after}"
    );
    let http = w.get_as(&w.skopanev, "address=note:kept").await?;
    ensure!(
        http["current_revision"] == r && hash(&http)? == http_hash,
        "{http}"
    );
    let mcp = object_as(&w, &w.skopanev, "note:kept", None).await?;
    ensure!(
        mcp["current_revision"] == r && hash(&mcp)? == mcp_hash,
        "{mcp}"
    );
    ensure!(object_history(&w, "note:kept").await? == history);
    Ok(())
}

async fn list_addresses(w: &World, token: &str, parent: &str) -> Result<Vec<String>> {
    let list = w
        .get_as(token, &format!("limit=100&parent={parent}"))
        .await?;
    Ok(list["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["address"].as_str().map(str::to_string))
        .collect())
}
