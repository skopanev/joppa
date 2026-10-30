// Part 7 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_requirement_without_criteria_reads_as_no_criteria_over_mcp() -> Result<()> {
    // req-has-criteria-ac2
    async fn read(w: &World, bot: &str, arguments: Value) -> Result<Value> {
        let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":arguments}})).send().await?;
        ensure!(response.status().is_success(), "{}", response.status());
        let response: Value = response.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        Ok(serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?)
    }
    /// The list item of `address` among the roots or the children of `parent`.
    async fn item(w: &World, bot: &str, parent: Option<&str>, address: &str) -> Result<Value> {
        let mut arguments = json!({"workspace":w.ws});
        if let Some(parent) = parent {
            arguments["parent"] = json!(parent);
        }
        let list = read(w, bot, arguments).await?;
        list["items"]
            .as_array()
            .and_then(|items| items.iter().find(|i| i["address"] == address).cloned())
            .ok_or_else(|| anyhow::anyhow!("{address} is not listed: {list}"))
    }
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["spec"] = types["requirement"].clone();
    types["spec"]["parents"] = json!(["*"]);
    types["spec"]["root"] = json!(true);
    types["condition"] = types["ac"].clone();
    types["condition"]["parents"] = json!(["spec"]);
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Test configured specification and criterion names"})).await?;
    for (kind, parent, criterion) in [
        ("requirement", Some("capability:empty"), "ac"),
        ("spec", None, "condition"),
    ] {
        w.command(json!({"op":"save_object","kind":kind,"id":"empty","revision":0,"parent":parent,"data":{"title":"The empty specification","body":"Define the exact behavior before work","owner":"owner","consumers":["customer"]},"reason":"Create a specification"})).await?;
        let address = format!("{kind}:empty");
        // No criteria: the list item has no children and a grey tone.
        let empty = item(&w, bot, parent, &address).await?;
        ensure!(
            empty.get("children").is_none() && empty["status"] == "grey",
            "{empty}"
        );
        w.command(json!({"op":"save_object","kind":criterion,"id":format!("{kind}-condition"),"revision":0,"parent":address,"data":{"title":"The defined behavior holds","owner":"owner","consumer":"customer","description":"Check the agreed behavior","depends_on":[],"before_launch":false,"decision_required":false},"reason":"Add the first criterion"})).await?;
        let current = item(&w, bot, parent, &address).await?;
        ensure!(current["children"] == 1, "{current}");
        // The saved empty revision still reads whole.
        let old = read(
            &w,
            bot,
            json!({"workspace":w.ws,"address":address,"revision":1}),
        )
        .await?;
        ensure!(
            old["revision"] == 1 && old["data"]["title"] == "The empty specification",
            "{old}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn returned_work_keeps_notes_and_partial_results_for_the_next_agent_after_holder_release()
-> Result<()> {
    // returned-work-kept-ac1
    returned_work_handover(false).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn returned_work_keeps_notes_and_partial_results_for_the_next_agent_after_owner_release()
-> Result<()> {
    // returned-work-kept-ac1
    returned_work_handover(true).await
}

/// One object read through MCP as `token`: the current revision, or exactly `revision`.
async fn object_as(w: &World, token: &str, address: &str, revision: Option<u32>) -> Result<Value> {
    let mut arguments = json!({"workspace":w.ws,"address":address});
    if let Some(revision) = revision {
        arguments["revision"] = json!(revision);
    }
    let result = mcp_call(w, token, "joppa_objects", arguments).await?;
    ensure!(result["isError"] != true, "{result}");
    Ok(serde_json::from_str(
        result["content"][0]["text"].as_str().unwrap_or("null"),
    )?)
}
