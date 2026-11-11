// Part 27 of tests/spec_http.rs, included there, so the test names stay unchanged.

// ac:acceptance-needs-green
// ac:help-matches-api-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn unknown_and_internal_operations_are_refused_without_writes() -> Result<()> {
    let w = world().await?;
    let before = saved_rows(&w).await?;
    // The valid first operation of each batch is an MCP operation: MCP hides member.
    let valid = json!({"op":"note","target":"requirement:key","id":"should-not-exist",
        "text":"A note that a refused batch never stores"});
    // The removed check-run operations name their replacement; the retired ones are
    // unknown. Both replay from the journal only.
    let retired = [
        "plan",
        "result",
        "finish",
        "accept",
        "revoke",
        "accept_commit",
        "issue",
        "resolve",
        "review_coverage",
        "define_verification_scope",
        "project",
        "withdraw_decision",
        "withdraw_source",
    ]
    .map(|op| json!({"op":op,"req":"key","revision":1}));
    for command in [
        json!({"op":"made_up_operation","id":"x"}),
        json!({"op":"migrate_notes"}),
        json!({"op":"enroll_member","id":"intruder"}),
    ]
    .into_iter()
    .chain(retired)
    {
        let expected = command["op"]
            .as_str()
            .filter(|op| joppa::removed_ops::NAMES.contains(op))
            .map_or("unknown operation".to_owned(), joppa::removed_ops::refusal);
        let (ok, http) = send_as(&w, &w.owner, &command).await?;
        ensure!(!ok && http["error"] == expected.as_str(), "{http}");
        let single = mcp_call(
            &w,
            &w.owner,
            "joppa_command",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
        )
        .await?;
        ensure!(
            single["isError"] == true && single["content"][0]["text"] == expected.as_str(),
            "{single}"
        );
        let batch = mcp_call(
            &w,
            &w.owner,
            "joppa_batch",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"operations":[{"command":valid},{"command":command}]}),
        )
        .await?;
        ensure!(
            batch["isError"] == true
                && batch["content"][0]["text"]
                    .as_str()
                    .is_some_and(|t| t.contains(expected.as_str())),
            "{batch}"
        );
    }
    // MCP refuses each hidden operation, alone and in a batch, with the fields that the full
    // catalog requires. HTTP takes them (restricted_operations_refuse_callers_...).
    for name in HIDDEN {
        let entry = joppa::commands::operation(name).context("catalog entry")?;
        let command = operation_samples::call(name, entry.required);
        let id = || uuid::Uuid::new_v4().to_string();
        let args = json!({"workspace":w.ws,"request_id":id(),"command":command});
        let single = mcp_call(&w, &w.owner, "joppa_command", args).await?;
        ensure!(
            single["isError"] == true && single["content"][0]["text"] == "unknown operation",
            "joppa_command {name}: {single}"
        );
        let operations = json!([{"command":valid},{"command":command}]);
        let args = json!({"workspace":w.ws,"request_id":id(),"operations":operations});
        let batch = mcp_call(&w, &w.owner, "joppa_batch", args).await?;
        ensure!(
            batch["isError"] == true
                && batch["content"][0]["text"] == "operation 2: unknown operation",
            "joppa_batch {name}: {batch}"
        );
    }
    ensure!(
        saved_rows(&w).await? == before,
        "a refused operation changed state"
    );
    let members = store::read(&w.pool, &w.ws, "owner", &store::Scope::full()).await?;
    ensure!(
        !members.to_string().contains("should-not-exist"),
        "the batch applied its valid operation"
    );
    Ok(())
}

/// Each caller that the help entry does not name is refused and changes nothing.
/// Then a named caller sends the same command and succeeds.
async fn check_callers(
    w: &World,
    help: &Value,
    holder: Option<&str>,
    command: Value,
    allowed: &str,
    refusal: &str,
) -> Result<String> {
    let name = command["op"].as_str().unwrap().to_owned();
    let callers: Vec<&str> = help["operations"][&name]["callers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let named = |token: &str| {
        let mut categories = vec!["member"];
        if token == w.bot {
            categories.push("agent");
        }
        // owner, alex and maria are the human members. bot is an agent.
        if token == w.owner || token == w.alex || token == w.maria {
            categories.push("human member");
        }
        if Some(token) == holder {
            categories.push("holder");
        }
        categories.iter().any(|c| callers.contains(c))
    };
    ensure!(named(allowed), "{name}: the allowed caller is not named");
    let before = saved_rows(w).await?;
    let mut refused = 0;
    for token in [&w.owner, &w.alex, &w.maria, &w.bot] {
        if named(token) {
            continue;
        }
        refused += 1;
        let (ok, reply) = send_as(w, token, &command).await?;
        ensure!(
            !ok && reply["error"]
                .as_str()
                .is_some_and(|e| e.contains(refusal) && !e.contains("owner")),
            "{name}: {reply}"
        );
    }
    ensure!(refused > 0, "{name} has no unnamed caller");
    ensure!(
        saved_rows(w).await? == before,
        "{name}: a refusal changed state"
    );
    let (ok, reply) = send_as(w, allowed, &command).await?;
    ensure!(ok, "{name}: {reply}");
    Ok(name)
}
