// Part 38 of tests/spec_http.rs, included there: dependencies have no rule by object type.

fn refused(reply: &(bool, Value), text: &str) -> bool {
    !reply.0 && reply.1["error"].as_str().is_some_and(|e| e.contains(text))
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
    own["relations"]["depends_on"]["edges"][0]["revision"] = pin;
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
