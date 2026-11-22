// Part 3 of tests/round2_mcp_and_restore_http.rs: ac:i2gz323e (nominal and off-nominal).

// ac:i2gz323e (nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn restore_object_restores_requirement_ac_test_and_task_resets_boundary_and_runs_in_batch()
-> Result<()> {
    let w = record_world::world(&["suite > check_a"]).await?;
    let tree = mcp_ok(&w, "joppa_tree", json!({})).await?;
    w.command(
        &w.owner,
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Add task type with stage and commit fields",
            "put_types": {
                "task": {
                    "title": "Tasks",
                    "role": "task",
                    "status": "own",
                    "parents": ["ac"],
                    "fields": {
                        "stage": {"type": "enum", "values": ["todo", "done"]},
                        "commit": {"type": "text"}
                    }
                }
            }
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    // Record an initial pass so T and A start green, add a link and a comment on A, and create + archive Task K.
    mcp_ok(
        &w,
        "joppa_record_results",
        json!({"commit": "1111111", "results": [{"test": "suite > check_a", "status": "pass"}]}),
    )
    .await?;
    ensure!(w.object("ac:a").await?["status"]["tone"] == "green");
    ok(
        &w,
        json!({"op": "save_object", "kind": "requirement", "id": "peer", "revision": 0, "parent": null, "data": {"title": "Peer", "owner": "owner", "body": "Peer"}, "reason": "Seed peer"}),
    )
    .await?;
    let a_rev0 = w.object("ac:a").await?["revision"].as_u64().unwrap();
    ok(
        &w,
        json!({"op": "save_object", "kind": "ac", "id": "a", "revision": a_rev0, "relations": {"related": {"edges": [{"to": "requirement:peer"}]}}, "reason": "Add link"}),
    )
    .await?;
    mcp_ok(
        &w,
        "joppa_record_results",
        json!({"commit": "2222222", "results": [{"test": "suite > check_a", "status": "pass"}]}),
    )
    .await?;
    ensure!(w.object("ac:a").await?["status"]["tone"] == "green");
    ok(
        &w,
        json!({"op": "note", "target": "ac:a", "id": "c-keep", "text": "Keep comment across archive"}),
    )
    .await?;
    ok(
        &w,
        json!({"op": "save_object", "kind": "task", "id": "k", "revision": 0, "parent": "ac:a", "data": {"title": "Task K", "owner": "owner", "stage": "done"}, "reason": "Create task"}),
    )
    .await?;

    // Archive K, T, A, and R in bottom-up order.
    let tests = mcp_ok(&w, "joppa_objects", json!({"parent": "ac:a", "kind": "test"})).await?;
    let t_addr = tests["items"][0]["address"].as_str().unwrap().to_owned();
    let t_rev = w.object(&t_addr).await?["revision"].as_u64().unwrap();
    ok(
        &w,
        json!({"op": "archive_object", "target": "task:k", "revision": 1, "reason": "Archive K"}),
    )
    .await?;
    ok(
        &w,
        json!({"op": "archive_object", "target": &t_addr, "revision": t_rev, "reason": "Archive T"}),
    )
    .await?;
    let a_rev = w.object("ac:a").await?["revision"].as_u64().unwrap();
    ok(
        &w,
        json!({"op": "archive_object", "target": "ac:a", "revision": a_rev, "reason": "Archive A"}),
    )
    .await?;
    let r_rev = w.object("requirement:r").await?["revision"].as_u64().unwrap();
    ok(
        &w,
        json!({"op": "archive_object", "target": "requirement:r", "revision": r_rev, "reason": "Archive R"}),
    )
    .await?;

    // Restore R (and transfer R owner to alex while A is still archived to test owner alignment), then restore A, T, and K.
    ok(
        &w,
        json!({"op": "restore_object", "target": "requirement:r", "revision": r_rev + 1, "reason": "Restore R"}),
    )
    .await?;
    w.command(
        &w.owner,
        json!({"op": "save_object", "kind": "requirement", "id": "r", "revision": r_rev + 2, "data": {"owner": "alex"}, "reason": "Transfer R owner"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    ok(
        &w,
        json!({"op": "restore_object", "target": "ac:a", "revision": a_rev + 1, "reason": "Restore A"}),
    )
    .await?;
    ok(
        &w,
        json!({"op": "restore_object", "target": &t_addr, "revision": t_rev + 1, "reason": "Restore T"}),
    )
    .await?;

    // Restored A keeps links, comments, history and original owner, and has a reset boundary.
    let restored_a = w.object("ac:a").await?;
    ensure!(
        restored_a["status"]["tone"] == "blue"
            && restored_a["data"]["owner"] == "owner"
            && restored_a["relations"]["related"]["edges"].as_array().is_some_and(|e| e.len() == 1)
            && restored_a["comments_count"] == 1,
        "restored A: {restored_a}"
    );
    mcp_ok(
        &w,
        "joppa_record_results",
        json!({"commit": "3333333", "results": [{"test": "suite > check_a", "status": "pass"}]}),
    )
    .await?;
    ensure!(w.object("ac:a").await?["status"]["tone"] == "green");

    // Restore K, archive K again, then repair K in one atomic batch [restore -> save -> archive].
    ok(&w, json!({"op": "restore_object", "target": "task:k", "revision": 2, "reason": "Restore K"})).await?;
    ensure!(w.object("task:k").await?["present"].is_null());
    ok(&w, json!({"op": "archive_object", "target": "task:k", "revision": 3, "reason": "Archive K again"})).await?;
    batch(
        &w,
        &[
            json!({"op": "restore_object", "target": "task:k", "revision": 4, "reason": "Reopen K"}),
            json!({"op": "save_object", "kind": "task", "id": "k", "revision": 5, "data": {"commit": "abcdef1"}, "reason": "Set commit"}),
            json!({"op": "archive_object", "target": "task:k", "revision": 6, "reason": "Archive repaired K"}),
        ],
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    replay(&w).await?;
    let k = w.object("task:k").await?;
    ensure!(
        k["present"] == false && k["revision"] == 7 && k["data"]["commit"] == "abcdef1",
        "repaired K: {k}"
    );
    Ok(())
}

// ac:i2gz323e (off-nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn restore_object_refuses_stale_revision_removed_type_and_archived_parent() -> Result<()> {
    let w = record_world::world(&["suite > check_a"]).await?;
    let tree = mcp_ok(&w, "joppa_tree", json!({})).await?;
    w.command(
        &w.owner,
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Add temporary task type job",
            "put_types": {"job": {"title": "Jobs", "role": "task", "status": "own", "parents": ["ac"]}}
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    ok(
        &w,
        json!({"op": "save_object", "kind": "job", "id": "j1", "revision": 0, "parent": "ac:a", "data": {"title": "Job", "owner": "owner"}, "reason": "Create job"}),
    )
    .await?;
    ok(
        &w,
        json!({"op": "archive_object", "target": "job:j1", "revision": 1, "reason": "Archive job"}),
    )
    .await?;
    let tests = mcp_ok(&w, "joppa_objects", json!({"parent": "ac:a", "kind": "test"})).await?;
    let t_addr = tests["items"][0]["address"].as_str().unwrap().to_owned();
    let t_rev = w.object(&t_addr).await?["revision"].as_u64().unwrap();
    ok(
        &w,
        json!({"op": "archive_object", "target": &t_addr, "revision": t_rev, "reason": "Archive test"}),
    )
    .await?;
    let a_rev = w.object("ac:a").await?["revision"].as_u64().unwrap();
    ok(
        &w,
        json!({"op": "archive_object", "target": "ac:a", "revision": a_rev, "reason": "Archive ac"}),
    )
    .await?;

    let before = head(&w).await?;
    refused(
        w.command(
            &w.bot,
            json!({"op": "restore_object", "target": &t_addr, "revision": t_rev + 1, "reason": "Parent archived"}),
        )
        .await?,
        "parent is absent from the current tree",
    )?;
    refused(
        w.command(
            &w.bot,
            json!({"op": "restore_object", "target": "ac:a", "revision": a_rev, "reason": "Stale"}),
        )
        .await?,
        "stale object revision",
    )?;
    ensure!(head(&w).await? == before);

    ok(
        &w,
        json!({"op": "restore_object", "target": "ac:a", "revision": a_rev + 1, "reason": "Restore ac"}),
    )
    .await?;
    let tree2 = mcp_ok(&w, "joppa_tree", json!({})).await?;
    w.command(
        &w.owner,
        json!({"op": "set_tree_schema", "revision": tree2["schema"]["revision"], "remove_types": ["job"], "reason": "Remove job type"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    let before2 = head(&w).await?;
    refused(
        w.command(
            &w.bot,
            json!({"op": "restore_object", "target": "job:j1", "revision": 2, "reason": "Removed type"}),
        )
        .await?,
        "unknown object type",
    )?;
    ensure!(head(&w).await? == before2);
    Ok(())
}
