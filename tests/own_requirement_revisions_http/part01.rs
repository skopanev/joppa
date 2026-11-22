// Part 1 of tests/own_requirement_revisions_http.rs: ac:drr28sx9 (nominal and off-nominal).

async fn drive_req(w: &World, id: &str, target: u64) -> Result<()> {
    let address = format!("requirement:{id}");
    loop {
        let rev = w.object(&address).await?["revision"]
            .as_u64()
            .unwrap_or(0);
        ensure!(rev <= target, "{address} at {rev} > {target}");
        if rev == target {
            return Ok(());
        }
        ok(
            w,
            json!({
                "op": "save_object",
                "kind": "requirement",
                "id": id,
                "revision": rev,
                "parent": null,
                "data": {"title": format!("Req {id}"), "owner": "owner", "body": format!("Body {rev}")},
                "reason": "Drive revision"
            }),
        )
        .await?;
    }
}

async fn read_req_state(w: &World, id: &str) -> Result<(i64, Value, Value, Value, i64)> {
    let address = format!("requirement:{id}");
    let one = mcp_ok(
        w,
        "joppa_objects",
        json!({"address": &address, "fields": ["status", "child_counts", "computed"]}),
    )
    .await?;
    let hist = mcp_ok(
        w,
        "joppa_objects",
        json!({"address": &address, "history": true}),
    )
    .await?;
    let rev = hist["current_revision"]
        .as_i64()
        .or_else(|| one["revision"].as_i64())
        .context("no revision")?;
    let total = hist["total"].as_i64().context("no history total")?;
    Ok((
        rev,
        one["status"].clone(),
        one["child_counts"].clone(),
        one["computed"]["stage"].clone(),
        total,
    ))
}

// ac:drr28sx9 (nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn criterion_writes_keep_requirement_revisions_update_rollups_and_replay_pre_epoch()
-> Result<()> {
    let w = record_world::world(&["suite > check_a", "suite > check_b"]).await?;
    let tree = mcp_ok(&w, "joppa_tree", json!({})).await?;
    let mut req_type = tree["schema"]["types"]["requirement"].clone();
    req_type["fields"] = json!({
        "stage": {
            "type": "enum",
            "values": ["to_prepare", "to_implement", "done"],
            "from": "status",
            "map": {"orange": "to_prepare", "blue": "to_implement", "green": "done"}
        }
    });
    w.command(
        &w.owner,
        json!({"op": "set_tree_schema", "revision": tree["schema"]["revision"], "put_types": {"requirement": req_type}, "reason": "Add computed stage on requirement"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    // Seed requirement:q at revision 3 and requirement:r at revision 5 with criteria a and b, both green.
    ok(
        &w,
        json!({"op": "save_object", "kind": "requirement", "id": "q", "revision": 0, "parent": null, "data": {"title": "Req q", "owner": "owner", "body": "Q0"}, "reason": "Create q"}),
    )
    .await?;
    drive_req(&w, "q", 3).await?;
    ok(
        &w,
        json!({"op": "save_object", "kind": "ac", "id": "b", "revision": 0, "parent": "requirement:r", "data": {"title": "B", "owner": "owner"}, "reason": "Create b"}),
    )
    .await?;
    drive_req(&w, "r", 5).await?;
    ok(
        &w,
        json!({"op": "save_object", "kind": "test", "id": "tb", "revision": 0, "parent": "ac:b", "data": {"title": "suite > check_b", "owner": "owner"}, "reason": "Bind test b"}),
    )
    .await?;
    mcp_ok(
        &w,
        "joppa_record_results",
        json!({"commit": "abcdef1", "results": [{"test": "suite > check_a", "status": "pass"}, {"test": "suite > check_b", "status": "pass"}]}),
    )
    .await?;

    let (r_rev, r_status, r_counts, r_stage, r_total) = read_req_state(&w, "r").await?;
    ensure!(
        r_rev == 5
            && r_total == 5
            && r_status == "green"
            && r_stage == "done"
            && r_counts == json!([{"kind": "ac", "total": 2, "passed": 2}]),
        "initial r: {r_rev} {r_status} {r_stage} {r_counts}"
    );
    let (q_rev, q_status, q_counts, q_stage, q_total) = read_req_state(&w, "q").await?;
    ensure!(
        q_rev == 3
            && q_total == 3
            && q_status == "orange"
            && q_stage == "to_prepare"
            && q_counts.is_null(),
        "initial q: {q_rev} {q_status} {q_stage} {q_counts}"
    );

    // 1. Create criterion c under r -> r stays at rev 5, status turns orange, child_counts total 3, stage to_prepare.
    ok(
        &w,
        json!({"op": "save_object", "kind": "ac", "id": "c", "revision": 0, "parent": "requirement:r", "data": {"title": "C", "owner": "owner"}, "reason": "Create c"}),
    )
    .await?;
    let (r_rev, r_status, r_counts, r_stage, r_total) = read_req_state(&w, "r").await?;
    ensure!(
        r_rev == 5
            && r_total == 5
            && r_status == "orange"
            && r_stage == "to_prepare"
            && r_counts == json!([{"kind": "ac", "total": 3, "passed": 2}]),
        "after create c: {r_rev} {r_status} {r_stage} {r_counts}"
    );

    // 2. Edit body of criterion a -> r stays at rev 5, passed drops to 1.
    let a_rev = w.object("ac:a").await?["revision"].as_u64().unwrap();
    ok(
        &w,
        json!({"op": "save_object", "kind": "ac", "id": "a", "revision": a_rev, "data": {"body": "Updated contract of A"}, "reason": "Edit a"}),
    )
    .await?;
    let (r_rev, _, r_counts, _, r_total) = read_req_state(&w, "r").await?;
    ensure!(
        r_rev == 5 && r_total == 5 && r_counts == json!([{"kind": "ac", "total": 3, "passed": 1}]),
        "after edit a: {r_rev} {r_counts}"
    );

    // 3. Move criterion b from r to q -> r stays at 5, q stays at 3, rollups of both update.
    let b_rev = w.object("ac:b").await?["revision"].as_u64().unwrap();
    ok(
        &w,
        json!({"op": "save_object", "kind": "ac", "id": "b", "revision": b_rev, "parent": "requirement:q", "reason": "Move b to q"}),
    )
    .await?;
    let (r_rev, _, r_counts, _, r_total) = read_req_state(&w, "r").await?;
    let (q_rev, q_status, q_counts, q_stage, q_total) = read_req_state(&w, "q").await?;
    ensure!(
        r_rev == 5 && r_total == 5 && r_counts == json!([{"kind": "ac", "total": 2, "passed": 0}]),
        "r after move b: {r_rev} {r_counts}"
    );
    ensure!(
        q_rev == 3
            && q_total == 3
            && q_status == "blue"
            && q_stage == "to_implement"
            && q_counts == json!([{"kind": "ac", "total": 1, "passed": 0}]),
        "q after move b: {q_rev} {q_status} {q_stage} {q_counts}"
    );

    // 4. Archive criterion c -> r stays at 5, status turns blue, stage turns to_implement, child_counts total 1.
    ok(
        &w,
        json!({"op": "archive_object", "target": "ac:c", "revision": 1, "reason": "Archive c"}),
    )
    .await?;
    let (r_rev, r_status, r_counts, r_stage, r_total) = read_req_state(&w, "r").await?;
    ensure!(
        r_rev == 5
            && r_total == 5
            && r_status == "blue"
            && r_stage == "to_implement"
            && r_counts == json!([{"kind": "ac", "total": 1, "passed": 0}]),
        "r after archive c: {r_rev} {r_status} {r_stage} {r_counts}"
    );

    // 5. Direct save of r with revision 5 succeeds at revision 6, and replay keeps r at 6 and q at 3.
    ok(
        &w,
        json!({"op": "save_object", "kind": "requirement", "id": "r", "revision": 5, "data": {"body": "Direct R update"}, "reason": "Save r directly"}),
    )
    .await?;
    let (r_rev, _, _, _, r_total) = read_req_state(&w, "r").await?;
    ensure!(r_rev == 6 && r_total == 6, "direct save r: {r_rev} {r_total}");
    replay(&w).await?;
    ensure!(revisions(&w, "requirement:r").await?.len() == 6);
    ensure!(revisions(&w, "requirement:q").await?.len() == 3);

    // Second fixture before own_requirement_revisions: replay keeps forced revisions on r and q.
    let pre = record_world::world(&[]).await?;
    ok(
        &pre,
        json!({"op": "save_object", "kind": "requirement", "id": "q", "revision": 0, "parent": null, "data": {"title": "Req q", "owner": "owner", "body": "Q0"}, "reason": "Create q"}),
    )
    .await?;
    drive_req(&pre, "q", 3).await?;
    // Move own_requirement_revisions past all events we are about to replay. Note that ac:a at setup also forced r from 1 -> 2 before the epoch, so drive r by appending pre-epoch events.
    let epoch_start = head(&pre).await? + 1;
    pre.pool
        .get()
        .await?
        .execute(
            "UPDATE joppa.replay_epochs SET first_position=$2 WHERE workspace=$1 AND rule='own_requirement_revisions'",
            &[&pre.ws, &(epoch_start + 100)],
        )
        .await?;
    replay(&pre).await?;
    // In pre, r is at revision 2 (1 create + 1 forced from setup ac:a). Append 3 direct saves of r -> rev 5, then create b (+1 on r -> 6), create c (+1 on r -> 7), edit a (+1 on r -> 8), move b to q (+1 on r -> 9, +1 on q -> 4), archive c (0 on r).
    for (rev, text) in [(2, "B2"), (3, "B3"), (4, "B4")] {
        append_event(
            &pre,
            "bot",
            &json!({"op": "save_object", "kind": "requirement", "id": "r", "revision": rev, "parent": null, "data": {"title": "Req r", "owner": "owner", "body": text}, "relations": {}, "reason": "Drive r"}),
        )
        .await?;
    }
    let a_pre_rev = pre.object("ac:a").await?["revision"].as_u64().unwrap();
    for event in [
        json!({"op": "save_object", "kind": "ac", "id": "b", "revision": 0, "parent": "requirement:r", "data": {"title": "B", "owner": "owner", "body": ""}, "relations": {}, "reason": "Create b"}),
        json!({"op": "save_object", "kind": "ac", "id": "c", "revision": 0, "parent": "requirement:r", "data": {"title": "C", "owner": "owner", "body": ""}, "relations": {}, "reason": "Create c"}),
        json!({"op": "save_object", "kind": "ac", "id": "a", "revision": a_pre_rev, "parent": "requirement:r", "data": {"title": "A", "owner": "owner", "body": "Pre-epoch A"}, "relations": {}, "reason": "Edit a"}),
        json!({"op": "save_object", "kind": "ac", "id": "b", "revision": 1, "parent": "requirement:q", "data": {"title": "B", "owner": "owner", "body": ""}, "relations": {}, "reason": "Move b"}),
        json!({"op": "archive_object", "target": "ac:c", "revision": 1, "reason": "Archive c"}),
    ] {
        append_event(&pre, "bot", &event).await?;
    }
    replay(&pre).await?;
    ensure!(revisions(&pre, "requirement:r").await?.len() == 8);
    ensure!(revisions(&pre, "requirement:q").await?.len() == 4);
    Ok(())
}

// ac:drr28sx9 (off-nominal)
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn direct_requirement_gap_stays_refused_after_criterion_edits() -> Result<()> {
    let w = record_world::world(&[]).await?;
    drive_req(&w, "r", 2).await?;
    ok(
        &w,
        json!({"op": "save_object", "kind": "requirement", "id": "r", "revision": 2, "data": {"body": "Revision 3 body"}, "reason": "Advance r"}),
    )
    .await?;
    let a_rev = w.object("ac:a").await?["revision"].as_u64().unwrap();
    ok(
        &w,
        json!({"op": "save_object", "kind": "ac", "id": "a", "revision": a_rev, "data": {"body": "Child edit"}, "reason": "Edit child"}),
    )
    .await?;
    ensure!(w.object("requirement:r").await?["revision"] == 3);
    let before = head(&w).await?;
    let stale = w
        .command(
            &w.bot,
            json!({"op": "save_object", "kind": "requirement", "id": "r", "revision": 2, "data": {"body": "Stale overwrite"}, "reason": "Stale"}),
        )
        .await?;
    refused(stale, "stale object revision")?;
    ensure!(head(&w).await? == before, "refused write appended an event");
    ensure!(w.object("requirement:r").await?["revision"] == 3);
    Ok(())
}
