// Part 18 of tests/reports_http.rs, included there, so the test names stay unchanged.

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_task_roles_read_their_criterion_results_over_http_and_mcp() -> Result<()> {
    async fn command(w: &World, value: Value) -> Result<()> {
        let (code, reply) = post(
            w,
            &w.owner,
            "/commands",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":value}),
        )
        .await?;
        ensure!(code == 200, "{value}: {code} {reply}");
        Ok(())
    }
    async fn read(w: &World, bot: &str, address: &str) -> Result<Value> {
        let response = w
            .http
            .get(format!("{}/tests?task={address}", w.base))
            .bearer_auth(bot)
            .send()
            .await?;
        let code = response.status().as_u16();
        let http: Value = response.json().await?;
        ensure!(code == 200, "{address}: {code} {http}");
        let root = w.base.split("/api/").next().unwrap();
        let workspace = w.base.rsplit('/').next().unwrap();
        let response: Value = w.http.post(format!("{root}/mcp")).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_tests","arguments":{"workspace":workspace,"task":address}}})).send().await?.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        let mcp: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        ensure!(
            compact_of::compact_of(&http, &mcp) == mcp,
            "HTTP {http}; MCP {mcp}"
        );
        Ok(http)
    }
    let w = world().await?;
    let schema: Value = w
        .http
        .get(format!("{}/tree", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut types = schema["schema"]["types"].clone();
    types["task"]["parents"] = json!(["*"]);
    types["job"] = types["task"].clone();
    types["job"]["parents"] = json!(["*"]);
    types["check"] = types["ac"].clone();
    types["check"]["parents"] = json!(["*"]);
    types["record"] = types["project"].clone();
    types["record"]["parents"] = json!(["*"]);
    command(&w,json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check configured task and criterion names"})).await?;
    command(&w,json!({"op":"save_object","kind":"check","id":"configured","revision":0,"parent":"requirement:key","data":{"title":"The configured result passes","description":"Check the configured contract","owner":"owner","consumer":"customer","depends_on":[],"before_launch":false,"decision_required":false},"reason":"Create a configured criterion"})).await?;
    command(&w,json!({"op":"set_ac_tests","target":"check:configured","tests":[A,B],"reason":"Bind the configured criterion"})).await?;
    for (kind, id, parent) in [
        ("job", "native", "ac:key-rights"),
        ("task", "configured", "check:configured"),
        ("job", "configured", "check:configured"),
    ] {
        command(&w,json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":{"title":"Implement the configured result","owner":"owner","contract":"Implement the agreed behavior and provide evidence","purpose":"implementation","projects":[]},"reason":"Check task result reads"})).await?;
    }
    command(&w,json!({"op":"save_object","kind":"record","id":"not-work","revision":0,"parent":"check:configured","data":{"title":"A document has no task role","owner":"owner"},"reason":"Check the role boundary"})).await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let tasks = [
        ("task:t1", "ac:key-rights"),
        ("job:native", "ac:key-rights"),
        ("task:configured", "check:configured"),
        ("job:configured", "check:configured"),
    ];
    for (commit, expected, verification, cases) in [
        (None, "grey", "unverified", vec![]),
        (
            Some("c1c1c1c"),
            "green",
            "passed",
            vec![("opens", "pass"), ("refuses", "pass")],
        ),
        (
            Some("c2c2c2c"),
            "red",
            "failed",
            vec![("opens", "fail"), ("refuses", "pass")],
        ),
    ] {
        if let Some(commit) = commit {
            w.record(commit, outcomes(&cases)).await?;
        }
        for (address, criterion) in tasks {
            // A configured Task role reads its own state: the criterion status does not
            // reach the Task.
            let _ = (criterion, expected, verification);
            let status = read(&w, bot, address).await?;
            ensure!(
                status["address"] == address
                    && status["done"] == false
                    && status["commit"].is_null()
                    && status.get("verification").is_none(),
                "{status}"
            );
        }
    }
    let response = w
        .http
        .get(format!("{}/tests?task=record:not-work", w.base))
        .bearer_auth(bot)
        .send()
        .await?;
    ensure!(response.status().as_u16() == 409, "{}", response.status());
    let refused: Value = response.json().await?;
    ensure!(refused["error"] == "unknown task", "{refused}");
    Ok(())
}

const BLOCKED: &str = "Task is blocked for planning";

const NO_MATCH: &str = "no available Task matches this claim";
