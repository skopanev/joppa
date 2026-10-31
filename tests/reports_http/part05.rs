// Part 5 of tests/reports_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn planning_claim_uses_the_current_batch_test_list_in_both_directions() -> Result<()> {
    let w = world().await?;
    w.list(&[A]).await?;
    ready_for_planning(&w).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "fail")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "the stored list must be green"
    );
    // check-list-frozen-ac3: each list save resets the criterion, so the batch assesses it again.
    let assess = json!({"op":"assess","req":"requirement:key","revision":2,"ac":"ac:key-rights","assessment":{"ready":true,"reason":"Known work scope"}});
    let before = planning_criterion(&w).await?;
    // The list save in the batch gives the criterion its next revision.
    let claim = json!({"op":"claim_planning","ac":"ac:key-rights","revision":before["current_revision"].as_u64().unwrap_or(0) + 1});
    let reply = planning_batch(&w, json!([
        {"command":{"op":"set_ac_tests","target":"ac:key-rights","tests":[A,B],"reason":"Include the failing check"}},
        {"command":assess},
        {"command":claim}
    ])).await?;
    ensure!(
        reply["result"]["isError"] != true && reply.get("error").is_none(),
        "current grey list must permit planning: {reply}"
    );
    ensure!(
        w.status().await?["status"] == "grey",
        "the new list must be grey"
    );
    ensure!(
        planning_criterion(&w).await?["process"]["planning_claim"]["actor"] == "agent",
        "claim missing"
    );
    let (code, reply) = post(&w, &w.token, "/commands", json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{
        "op":"release_planning","ac":"ac:key-rights","reason":"Check the opposite batch transition"}})).await?;
    ensure!(code == 200, "{code} {reply}");
    let before = planning_criterion(&w).await?;
    // The list save in the batch gives the criterion its next revision.
    let claim = json!({"op":"claim_planning","ac":"ac:key-rights","revision":before["current_revision"].as_u64().unwrap_or(0) + 1});
    let reply = planning_batch(&w, json!([
        {"command":{"op":"set_ac_tests","target":"ac:key-rights","tests":[A],"reason":"Use the passing check"}},
        {"command":assess},
        {"command":claim}
    ])).await?;
    ensure!(
        reply["result"]["isError"] != true && reply.get("error").is_none(),
        "the earlier passing report must not refuse planning: {reply}"
    );
    ensure!(
        w.status().await?["status"] == "grey",
        "the earlier report must not count after the list save"
    );
    ensure!(
        planning_criterion(&w).await?["process"]["planning_claim"]["actor"] == "agent",
        "claim missing"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_tree_counts_ready_direct_children_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let command = |value: Value| {
        let w = &w;
        async move {
            let (code, reply) = post(
                w,
                &w.owner,
                "/commands",
                json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":value}),
            )
            .await?;
            ensure!(code == 200, "{code}: {reply}");
            Ok::<_, anyhow::Error>(())
        }
    };
    let tree: Value = w
        .http
        .get(format!("{}/tree", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    let mut types = tree["schema"]["types"].clone();
    for (kind, role, parent, title) in [
        ("bundle", "group", None, "Bundles"),
        ("branch", "group", Some("bundle"), "Branches"),
        ("sheet", "requirement", Some("branch"), "Sheets"),
        ("proof", "ac", Some("sheet"), "Proofs"),
        ("extra", "document", Some("bundle"), "Extra"),
    ] {
        let status = match role {
            "ac" => "tests",
            "group" | "requirement" => "children",
            _ => "none",
        };
        types[kind] = json!({"title":title,"role":role,"status":status,"root":parent.is_none(),"parents":parent.into_iter().collect::<Vec<_>>()});
    }
    command(json!({"op":"set_tree_schema","revision":tree["current_revision"],"types":types,"reason":"Count configured tree levels"})).await?;
    for (kind, id, parent) in [
        ("bundle", "root", None),
        ("branch", "good", Some("bundle:root")),
        ("branch", "bad", Some("bundle:root")),
        ("branch", "empty", Some("bundle:root")),
        ("sheet", "one", Some("branch:good")),
        ("sheet", "two", Some("branch:good")),
        ("sheet", "three", Some("branch:bad")),
        ("extra", "input", Some("bundle:root")),
    ] {
        command(json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":{"title":id,"body":"The contract","owner":"owner"},"reason":"Create the counted tree"})).await?;
    }
    for (id, parent, test) in [
        ("a", "sheet:one", A),
        ("b", "sheet:one", A),
        ("c", "sheet:two", A),
        ("d", "sheet:three", B),
    ] {
        command(json!({"op":"save_object","kind":"proof","id":id,"revision":0,"parent":parent,"data":{"title":id,"body":"The exact criterion","owner":"owner"},"reason":"Create a configured criterion"})).await?;
        command(json!({"op":"set_ac_tests","target":format!("proof:{id}"),"tests":[test],"reason":"Bind its exact test"})).await?;
    }
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "fail")]),
    )
    .await?;
    for (address, total, passed, failed) in [
        ("bundle:root", 3, 1, 1),
        ("branch:good", 2, 2, 0),
        ("branch:bad", 1, 0, 1),
        ("branch:empty", 0, 0, 0),
        ("sheet:one", 2, 2, 0),
    ] {
        let http: Value = w
            .http
            .get(format!("{}/objects?address={address}", w.base))
            .bearer_auth(&w.owner)
            .send()
            .await?
            .json()
            .await?;
        let root = w.base.split("/api/").next().unwrap();
        let workspace = w.base.rsplit('/').next().unwrap();
        let mcp: Value = w.http.post(format!("{root}/mcp")).bearer_auth(&w.owner).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":workspace,"address":address}}})).send().await?.json().await?;
        let mcp: Value = serde_json::from_str(
            mcp["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        let expected = json!({"total":total,"passed":passed,"failed":failed});
        // MCP gives only the counts above 0.
        let mut counts = expected.clone();
        if let Some(m) = counts.as_object_mut() {
            m.retain(|_, n| *n != 0);
        }
        let counts = if counts == json!({}) { Value::Null } else { counts };
        ensure!(
            http["verification_summary"] == expected && mcp["verification_summary"] == counts,
            "{address}: HTTP {http}, MCP {mcp}"
        );
    }
    let list: Value = w
        .http
        .get(format!("{}/objects?kind=bundle", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    ensure!(
        list["items"][0]["verification_summary"] == json!({"total":3,"passed":1,"failed":1}),
        "{list}"
    );
    command(json!({"op":"archive_object","target":"branch:empty","revision":1,"reason":"Remove an unused branch"})).await?;
    let after: Value = w
        .http
        .get(format!("{}/objects?address=bundle:root", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    // check-list-frozen-ac3: the archive resets every criterion below bundle:root.
    ensure!(
        after["verification_summary"] == json!({"total":2,"passed":0,"failed":0}),
        "{after}"
    );
    Ok(())
}

// status-current-only-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_listed_test_without_a_result_is_not_run_and_grey() -> Result<()> {
    // A lists opens (X) and refuses (Y); the only record of C with X passing.
    let w = world().await?;
    w.record("c1c1c1c", outcomes(&[("opens", "pass")])).await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let reply = w
        .http
        .get(format!("{}/tests?ac=ac:key-rights", w.base))
        .bearer_auth(bot)
        .send()
        .await?;
    let code = reply.status();
    let text = reply.text().await?;
    ensure!(code.is_success(), "{code} {text}");
    let s: Value = serde_json::from_str(&text)?;
    let test = |address: &str| {
        s["tests"]
            .as_array()
            .and_then(|tests| tests.iter().find(|t| t["address"] == address))
    };
    ensure!(
        test(A).is_some_and(|x| x["status"] == "pass" && x["commit"] == "c1c1c1c")
            && test(B).is_some_and(|y| y["status"] == "not_run")
            && s["status"] == "grey"
            && s["reason"] == "not all tests passed",
        "{s}"
    );
    Ok(())
}
