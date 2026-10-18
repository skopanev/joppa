// Part 7 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_requirement_without_criteria_reads_as_no_criteria_over_mcp() -> Result<()> {
    // req-has-criteria-ac2
    async fn read(w: &World, bot: &str, arguments: Value) -> Result<Value> {
        let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_read","arguments":arguments}})).send().await?;
        ensure!(response.status().is_success(), "{}", response.status());
        let response: Value = response.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        Ok(serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?)
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
        w.command(json!({"op":"save_object","kind":kind,"id":"empty","revision":0,"parent":parent,"data":{"title":"The empty specification","body":"Define the exact behavior before work","owner":"owner","consumers":["customer"]},"reason":"Create a specification without criteria"})).await?;
        let address = format!("{kind}:empty");
        let object = w.get(&format!("address={address}")).await?;
        let item_id = object["item_id"].clone();
        for arguments in [
            json!({"workspace":w.ws,"req":address}),
            json!({"item_id":item_id}),
        ] {
            let empty = read(&w, bot, arguments).await?;
            ensure!(empty["status"] == "no criteria", "{empty}");
            ensure!(empty["verification"] == "unverified", "{empty}");
            ensure!(
                empty["requirement"]["revisions"][0]["acs"]
                    .as_array()
                    .is_some_and(|acs| acs.is_empty()),
                "{empty}"
            );
            ensure!(
                empty["summary"]["accepted"] == false
                    && empty["summary"]["ready_for_acceptance"] == false,
                "{empty}"
            );
        }
        let history = read(
            &w,
            bot,
            json!({"workspace":w.ws,"req":address,"history":true}),
        )
        .await?;
        ensure!(
            history.get("status").is_none() && history["requirement"].get("revisions").is_none(),
            "Counts-only history included document status: {history}"
        );
        w.command(json!({"op":"save_object","kind":criterion,"id":format!("{kind}-condition"),"revision":0,"parent":address,"data":{"title":"The defined behavior holds","owner":"owner","description":"Check the agreed behavior","depends_on":[],"before_launch":false,"decision_required":false},"reason":"Add the first criterion"})).await?;
        let current = read(&w, bot, json!({"workspace":w.ws,"req":address})).await?;
        ensure!(
            current["status"] != "no criteria",
            "The current nonempty revision still has no criteria: {current}"
        );
        ensure!(
            current["requirement"]["revisions"][0]["acs"]
                .as_array()
                .is_some_and(|acs| acs.len() == 1),
            "{current}"
        );
        for arguments in [
            json!({"workspace":w.ws,"req":address,"revision":1}),
            json!({"item_id":item_id,"revision":1}),
        ] {
            let old = read(&w, bot, arguments).await?;
            ensure!(
                old["status"] == "no criteria" && old["verification"] == "unverified",
                "The saved empty revision lost its status: {old}"
            );
            ensure!(old["requirement"]["revisions"][0]["number"] == 1, "{old}");
        }
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
