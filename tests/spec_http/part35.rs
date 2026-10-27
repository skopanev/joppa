#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_completed_prerequisite_task_unblocks_its_dependent_over_mcp() -> Result<()> {
    // pins-ignore-process-events-ac1: Task B has a blocking depends_on link to Task A.
    let w = world().await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let a = ready_task(&w, "a", "key-rights").await?;
    let object = async |address: &str| {
        mcp_value(&w, "joppa_objects", json!({"workspace":w.ws,"address":address})).await
    };
    let pin = object(&a).await?["current_revision"].clone();
    w.command(json!({"op":"save_object","kind":"task","id":"b","revision":0,"parent":"ac:key-rights","data":{"title":"Use the prerequisite work","owner":"owner","contract":"Use the completed prerequisite and record the result","purpose":"implementation","projects":[]},"links":[{"target":a,"relation":"depends_on","revision":pin,"blocking":true}],"reason":"Depend on the prerequisite"})).await?;
    let available = async || {
        let page = mcp_value(&w, "joppa_tasks", json!({"workspace":w.ws,"view":"available"})).await?;
        anyhow::Ok(
            page["tasks"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|row| row["address"] == "task:b"),
        )
    };
    ensure!(!available().await?, "B waits for A");
    // Agent bot claims A and completes it with evidence over MCP.
    let command = async |value: Value| {
        mcp_value(&w, "joppa_command", json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":value})).await
    };
    command(json!({"op":"claim_task","id":a,"revision":object(&a).await?["current_revision"]})).await?;
    command(json!({"op":"complete_task","commit":"c0ffee0","id":a,"revision":object(&a).await?["current_revision"],"evidence":"Implemented the prerequisite"})).await?;
    let done = object(&a).await?;
    ensure!(done["process"]["status"] == "done", "{done}");
    ensure!(available().await?, "B is available after A is done");
    let b = object("task:b").await?;
    ensure!(
        b["review_required"] == false && b["document"]["links"][0]["revision"] == pin,
        "{b}"
    );
    Ok(())
}
