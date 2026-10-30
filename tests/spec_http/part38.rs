// Part 38 of tests/spec_http.rs, included there: dependencies have no rule by object type.

/// The owner saves criterion `id` under `req` with `depends_on` and blocking `links`.
async fn save_criterion(
    w: &World,
    id: &str,
    req: &str,
    depends_on: Value,
    links: Value,
) -> Result<(bool, Value)> {
    let revision = w.get(&format!("address=ac:{id}")).await?["current_revision"].clone();
    let data = json!({"title":format!("Criterion {id}"),"owner":"owner","consumer":"customer",
        "before_launch":false,"decision_required":false,"depends_on":depends_on});
    let command = json!({"op":"save_object","kind":"ac","id":id,"revision":revision,
        "parent":format!("requirement:{req}"),"data":data,"links":links,"reason":"Order the work"});
    send_as(w, &w.owner, &command).await
}

/// Bot claims `task` at its current revision.
async fn claim(w: &World, task: &str) -> Result<(bool, Value)> {
    let revision = w.get(&format!("address={task}")).await?["current_revision"].clone();
    send_as(
        w,
        &w.bot,
        &json!({"op":"claim_task","id":task,"revision":revision}),
    )
    .await
}

/// Bot claims and completes `task`.
async fn finish(w: &World, task: &str) -> Result<()> {
    let (ok, claimed) = claim(w, task).await?;
    ensure!(ok, "{task}: {claimed}");
    let revision = w.get(&format!("address={task}")).await?["current_revision"].clone();
    w.command_as(&w.bot, json!({"op":"complete_task","commit":"c0ffee0","id":task,
        "revision":revision,"evidence":"Implemented the contract"}))
        .await?;
    Ok(())
}

fn refused(reply: &(bool, Value), text: &str) -> bool {
    !reply.0 && reply.1["error"].as_str().is_some_and(|e| e.contains(text))
}

// dependency-types-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_criterion_that_depends_on_a_task_waits_until_the_task_is_done() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    let link = json!([{"target":"task:t1","relation":"depends_on","blocking":true}]);
    let saved = save_criterion(&w, "other-audit", "other", json!([]), link).await?;
    ensure!(saved.0, "{}", saved.1);
    w.assess_ready("other-audit").await?;
    ready_task(&w, "t4", "other-audit").await?;
    let early = claim(&w, "task:t4").await?;
    ensure!(refused(&early, "waits for task:t1"), "{early:?}");
    finish(&w, "task:t1").await?;
    let late = claim(&w, "task:t4").await?;
    ensure!(late.0, "{late:?}");
    Ok(())
}

// dependency-types-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_that_depends_on_a_requirement_waits_for_its_criteria() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    let test = "tests/key.rs > rights";
    w.command(json!({"op":"set_ac_tests","target":"ac:key-rights","tests":[test],"reason":"Bind"}))
        .await?;
    let pin = w.get("address=requirement:key").await?["current_revision"].clone();
    let mut command = linked_task_command("t2", 1, "other-rights", "requirement:key");
    command["links"][0]["revision"] = pin;
    let (ok, saved) = send_as(&w, &w.owner, &command).await?;
    ensure!(ok, "{saved}");
    let early = claim(&w, "task:t2").await?;
    ensure!(refused(&early, "waits for requirement:key"), "{early:?}");
    let results = json!({"workspace":w.ws,"commit":"c0ffee0","results":[{"test":test,"status":"pass"}]});
    let recorded = mcp_call(&w, &w.bot, "joppa_record_results", results).await?;
    ensure!(recorded["isError"] != true, "{recorded}");
    let late = claim(&w, "task:t2").await?;
    ensure!(late.0, "{late:?}");
    Ok(())
}

// dependency-types-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_dependency_cycle_through_tasks_is_refused_like_any_other() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    let saved = w.get("address=task:t1").await?;
    // A Task waits on its own requirement: the requirement needs the Task's criterion.
    let pin = w.get("address=requirement:key").await?["current_revision"].clone();
    let mut own = linked_task_command("t1", 1, "key-rights", "requirement:key");
    own["links"][0]["revision"] = pin;
    let reply = send_as(&w, &w.owner, &own).await?;
    ensure!(refused(&reply, "dependency cycle"), "{reply:?}");
    // T2 waits on T1, so T1 cannot wait on T2.
    let (ok, linked) = send_as(
        &w,
        &w.owner,
        &linked_task_command("t2", 1, "other-rights", "task:t1"),
    )
    .await?;
    ensure!(ok, "{linked}");
    let back = linked_task_command("t1", 1, "key-rights", "task:t2");
    let reply = send_as(&w, &w.owner, &back).await?;
    ensure!(refused(&reply, "dependency cycle"), "{reply:?}");
    let after = w.get("address=task:t1").await?;
    ensure!(after["document"] == saved["document"], "{after}");
    Ok(())
}

// dependency-types-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_criterion_depends_on_a_criterion_of_another_requirement() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    w.requirement("third", "restrict", "Third", "Scope", &[("third-check", "Check the third")])
        .await?;
    let saved = save_criterion(&w, "other-audit", "other", json!(["third-check"]), json!([])).await?;
    ensure!(saved.0, "{}", saved.1);
    let audit = w.get("address=ac:other-audit").await?;
    ensure!(
        audit["document"]["data"]["depends_on"] == json!(["third-check"]),
        "{audit}"
    );
    w.assess_ready("other-audit").await?;
    ready_task(&w, "t4", "other-audit").await?;
    // The dependency of another requirement has no readiness assessment yet.
    let early = claim(&w, "task:t4").await?;
    ensure!(refused(&early, "third-check"), "{early:?}");
    w.assess_ready("third-check").await?;
    let late = claim(&w, "task:t4").await?;
    ensure!(late.0, "{late:?}");
    // A cycle across requirements is refused.
    let back = save_criterion(&w, "third-check", "third", json!(["other-audit"]), json!([])).await?;
    ensure!(refused(&back, "AC dependency cycle"), "{back:?}");
    Ok(())
}
