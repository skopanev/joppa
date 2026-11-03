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

/// The target that `node` waits for, as a read gives it.
async fn waits(w: &World, node: &str) -> Result<Option<String>> {
    let read = w.get(&format!("address={node}")).await?;
    Ok(read["waits_for"]["address"].as_str().map(String::from))
}

/// The owner archives `task`: its work is done.
async fn finish(w: &World, task: &str) -> Result<()> {
    let revision = w.get(&format!("address={task}")).await?["current_revision"].clone();
    w.command_as(&w.owner, json!({"op":"archive_object","target":task,"revision":revision,
        "reason":"Implemented the contract"}))
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
    ensure!(waits(&w, "task:t4").await?.as_deref() == Some("task:t1"), "t4 waits");
    finish(&w, "task:t1").await?;
    ensure!(waits(&w, "task:t4").await?.is_none(), "t4 is free");
    Ok(())
}

// dependency-types-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_that_depends_on_a_requirement_waits_for_its_criteria() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    let test = "tests/key.rs > rights";
    bind_tests(&w, &w.owner, "ac:key-rights", &[test]).await?;
    let pin = w.get("address=requirement:key").await?["current_revision"].clone();
    let mut command = linked_task_command("t2", 1, "other-rights", "requirement:key");
    command["links"][0]["revision"] = pin;
    let (ok, saved) = send_as(&w, &w.owner, &command).await?;
    ensure!(ok, "{saved}");
    let early = waits(&w, "task:t2").await?;
    ensure!(early.as_deref() == Some("requirement:key"), "{early:?}");
    let results = json!({"workspace":w.ws,"commit":"c0ffee0","results":[{"test":test,"status":"pass"}]});
    let recorded = mcp_call(&w, &w.bot, "joppa_record_results", results).await?;
    ensure!(recorded["isError"] != true, "{recorded}");
    ensure!(waits(&w, "task:t2").await?.is_none(), "t2 is free");
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
