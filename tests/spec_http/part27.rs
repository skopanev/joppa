// Part 27 of tests/spec_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn unknown_and_internal_operations_are_refused_without_writes() -> Result<()> {
    let w = world().await?;
    let before = saved_rows(&w).await?;
    let valid = json!({"op":"member","id":"should-not-exist","role":"human"});
    // The retired check-run and typed operations replay from the journal only.
    let retired = [
        "plan",
        "result",
        "finish",
        "accept",
        "revoke",
        "issue",
        "resolve",
        "review_coverage",
        "define_verification_scope",
        "edit_consumer",
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
        let (ok, http) = send_as(&w, &w.owner, &command).await?;
        ensure!(!ok && http["error"] == "unknown operation", "{http}");
        let single = mcp_call(
            &w,
            &w.owner,
            "joppa_command",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
        )
        .await?;
        ensure!(
            single["isError"] == true && single["content"][0]["text"] == "unknown operation",
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
                    .is_some_and(|t| t.contains("unknown operation")),
            "{batch}"
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
        if token == w.owner {
            categories.push("workspace owner");
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
            !ok && reply["error"].as_str().is_some_and(|e| e.contains(refusal)),
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
