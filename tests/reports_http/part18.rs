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
        let http = task_process(w, bot, address, false).await?;
        let mcp = task_process(w, bot, address, true).await?;
        // MCP gives the same state and commit as HTTP.
        ensure!(
            http["status"] == mcp["status"] && http["commit"] == mcp["commit"],
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
    // A test node may stand under the configured criterion type.
    types["test"]["parents"] = json!(["ac", "check"]);
    types["record"] = types["project"].clone();
    types["record"]["parents"] = json!(["*"]);
    command(&w,json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check configured task and criterion names"})).await?;
    command(&w,json!({"op":"save_object","kind":"check","id":"configured","revision":0,"parent":"requirement:key","data":{"title":"The configured result passes","body":"Check the configured contract","owner":"owner","consumer":"customer","depends_on":[],"before_launch":false,"decision_required":false},"reason":"Create a configured criterion"})).await?;
    bind_at(&w.http, &w.base, &w.owner, "check:configured", &[A, B]).await?;
    for (kind, id, parent) in [
        ("job", "native", "ac:key-rights"),
        ("task", "configured", "check:configured"),
        ("job", "configured", "check:configured"),
    ] {
        command(&w,json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":{"title":"Implement the configured result","owner":"owner","body":"Implement the agreed behavior and provide evidence","projects":[]},"reason":"Check task result reads"})).await?;
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
                status["status"] == "open"
                    && status["commit"].is_null()
                    && status.get("verification").is_none(),
                "{status}"
            );
        }
    }
    Ok(())
}

const BLOCKED: &str = "Task is blocked for planning";

const NO_MATCH: &str = "no available Task matches this claim";
