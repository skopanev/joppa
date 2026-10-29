// Part 22 of tests/reports_http.rs, included there, so the test names stay unchanged.

/// Saved rows without the planning claim, the journal and its derived rows.
fn without_claim(mut saved: Value) -> Value {
    for table in ["events", "command_receipts", "board_cache", "position"] {
        saved[table] = Value::Null;
    }
    for table in ["state_entries", "state_revisions"] {
        saved[table]
            .as_array_mut()
            .unwrap()
            .retain(|row| row["section"] != "planning_claims");
    }
    for object in saved["objects"].as_array_mut().unwrap() {
        if object["process"].is_object() {
            object["process"]["planning_claim"] = Value::Null;
        }
    }
    saved
}

// planning-claim-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac6_fixture1_a_holder_test_list_save_finishes_with_no_task_and_replays() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.bound_plan().await?;
        let objects = p.addresses().await?;
        p.send(mcp, &save_xy()).await?.map_err(anyhow::Error::msg)?;
        p.send(mcp, &finish_command())
            .await?
            .map_err(anyhow::Error::msg)?;
        let a = p.read("criterion:a").await?;
        ensure!(a["process"]["planning_claim"].is_null(), "{a}");
        ensure!(p.addresses().await? == objects, "finish created a Task");
        ensure!(listed(&p.tests().await?) == [A, B]);
        store::check_journal(&p.pool).await?;
        p.pool
            .get()
            .await?
            .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&p.ws])
            .await?;
        store::migrate(&p.pool).await?;
        ensure!(p.read("criterion:a").await?["process"]["planning_claim"].is_null());
        ensure!(listed(&p.tests().await?) == [A, B]);
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// planning-claim-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac6_fixture2_a_finish_without_changes_is_refused_and_the_holder_releases() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.bound_plan().await?;
        p.refuse_command("no change", finish_command(), NO_TASK_CHANGES)
            .await?;
        ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "agent");
        ensure!(listed(&p.tests().await?) == [A]);
        let before = p.saved().await?;
        p.send(
            mcp,
            &json!({"op":"release_planning","ac":"criterion:a","reason":"Nothing to plan"}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
        let after = p.saved().await?;
        ensure!(p.read("criterion:a").await?["process"]["planning_claim"].is_null());
        ensure!(
            without_claim(after) == without_claim(before),
            "release changed more than the claim"
        );
    }
    Ok(())
}

// planning-claim-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac6_fixture3_a_test_list_save_by_another_member_does_not_finish() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.bound_plan().await?;
    p.write("other", save_xy()).await?;
    p.refuse_command("save by Q", finish_command(), NO_TASK_CHANGES)
        .await?;
    ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "agent");
    ensure!(listed(&p.tests().await?) == [A, B]);
    Ok(())
}

// planning-claim-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac6_fixture4_a_refused_batch_rolls_back_the_save_and_the_finish() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.bound_plan().await?;
    let before = p.saved().await?;
    let response = p
        .mcp(
            "joppa_batch",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"operations":[
                {"command":save_xy()},
                {"command":finish_command()},
                {"command":{"op":"set_ac_tests","target":"criterion:missing","tests":[A],"reason":"Refused"}}
            ]}),
        )
        .await?;
    ensure!(
        response["result"]["isError"] == true
            && response["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|e| e.ends_with("unknown criterion")),
        "{response}"
    );
    let after = p.saved().await?;
    ensure!(
        after == before && after["position"] == before["position"],
        "the refused batch changed saved state"
    );
    ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "agent");
    ensure!(listed(&p.tests().await?) == [A]);
    Ok(())
}

const AWAITING: &str = "Task is awaiting planning publication";

const HOLDER_REQUIRED: &str = "planning holder required";

const NO_CLAIM: &str = "criterion has no planning claim";

const EMPTY_TEXT: &str = "text must not be empty";
