// Part 30 of tests/spec_http.rs, included there, so the test names stay unchanged.

/// REQ key holds task:t1; REQ other holds task:t2 under other-rights and a second AC other-audit.
async fn tasks_under_two_requirements(w: &World) -> Result<()> {
    w.requirement(
        "other",
        "restrict",
        "Other rights",
        "Scope",
        &[
            ("other-rights", "Only write after review"),
            ("other-audit", "Record each write"),
        ],
    )
    .await?;
    ready_task(w, "t1", "key-rights").await?;
    ready_task(w, "t2", "other-rights").await?;
    Ok(())
}

/// A Task save with one depends_on edge to `target` at revision 1.
fn linked_task_command(id: &str, revision: u32, ac: &str, target: &str) -> Value {
    let mut command = ready_task_command(id, ac);
    command["revision"] = json!(revision);
    command["relations"] = json!({"depends_on":{"edges":[{"to":target,"revision":1}]}});
    command
}

/// The HTTP read gives `document.relations.depends_on.edges` = [{to: target, revision: 1}],
/// with no blocking field.
fn depends_on_pinned(read: &Value, target: &str) -> bool {
    read["document"]["relations"]["depends_on"]["edges"] == json!([{"to":target,"revision":1}])
}

// task-links-stay-in-req-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_link_to_a_task_of_another_requirement_is_saved() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    // Bot creates T3 under R2 with a link to T1 over HTTP; the owner links T2 over MCP.
    let (ok, created) = send_as(
        &w,
        &w.bot,
        &linked_task_command("t3", 0, "other-rights", "task:t1"),
    )
    .await?;
    ensure!(ok, "{created}");
    let command = linked_task_command("t2", 1, "other-rights", "task:t1");
    let request = uuid::Uuid::new_v4().to_string();
    let args = json!({"workspace":w.ws,"request_id":request,"command":command});
    let mcp = mcp_call(&w, &w.owner, "joppa_command", args).await?;
    ensure!(mcp["isError"] != true, "{mcp}");
    // Each dependent Task keeps the link.
    for task in ["task:t2", "task:t3"] {
        let read = w.get(&format!("address={task}")).await?;
        ensure!(depends_on_pinned(&read, "task:t1"), "{read}");
    }
    Ok(())
}

// task-links-stay-in-req-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_link_across_criteria_of_one_requirement_is_saved() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    w.command_as(
        &w.bot,
        linked_task_command("t4", 0, "other-audit", "task:t2"),
    )
    .await?;
    let t4 = w.get_as(&w.bot, "address=task:t4").await?;
    ensure!(depends_on_pinned(&t4, "task:t2"), "{t4}");
    Ok(())
}

// task-links-stay-in-req-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_link_added_across_criteria_of_one_requirement_is_saved() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    // Bot creates task:t4 so that bot, as its unclaimed creator, may add the link.
    w.command_as(&w.bot, ready_task_command("t4", "other-audit"))
        .await?;
    let t4 = "task:t4";
    let (ok, saved) = send_as(
        &w,
        &w.bot,
        &linked_task_command("t4", 1, "other-audit", "task:t2"),
    )
    .await?;
    ensure!(ok, "{saved}");
    let read = w.get_as(&w.bot, &format!("address={t4}")).await?;
    ensure!(depends_on_pinned(&read, "task:t2"), "{read}");
    Ok(())
}

// task-links-stay-in-req-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_journal_with_a_cross_requirement_task_link_still_replays() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    // An edge stored in the journal before the live save replays to the same document.
    let command = linked_task_command("t2", 1, "other-rights", "task:t1");
    let position = journal_position(&w).await? + 1;
    let conn = w.pool.get().await?;
    conn.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,'owner',$3,$4::text::jsonb)",
        &[&w.ws, &position, &uuid::Uuid::new_v4().to_string(), &command.to_string()],
    )
    .await?;
    conn.execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&w.ws])
        .await?;
    store::backfill_board(&w.pool).await?;
    let t2 = w.get("address=task:t2").await?;
    ensure!(
        t2["current_revision"] == 2 && depends_on_pinned(&t2, "task:t1"),
        "{t2}"
    );
    Ok(())
}
