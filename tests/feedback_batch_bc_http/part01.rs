// Part 1 of tests/feedback_batch_bc_http.rs: ac:ua662yvd, ac:szu8fkbc, ac:l6drwbo5, ac:pquuuvss.

// ac:ua662yvd
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn commit_required_false_takes_records_without_commit_and_default_refuses() -> Result<()> {
    let w1 = record_world::world(&["tests/x.rs > manual_metric"]).await?;
    let w2 = record_world::world(&["tests/x.rs > code_test"]).await?;

    let tree = ok(&w1, "joppa_tree", json!({})).await?;
    let mut tr = tree["schema"]["test_results"].clone();
    tr["commit_required"] = json!(false);
    w1.command(
        &w1.owner,
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "test_results": tr,
            "reason": "Allow records without git commit"
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    // Record fail without commit, then pass without commit, then pass with empty commit.
    ok(
        &w1,
        "joppa_record_results",
        json!({"results": [{"test": "tests/x.rs > manual_metric", "status": "fail"}]}),
    )
    .await?;
    ensure!(w1.object("ac:a").await?["status"]["tone"] == "red");

    ok(
        &w1,
        "joppa_record_results",
        json!({"results": [{"test": "tests/x.rs > manual_metric", "status": "pass"}]}),
    )
    .await?;
    ensure!(w1.object("ac:a").await?["status"]["tone"] == "green");

    ok(
        &w1,
        "joppa_record_results",
        json!({"commit": "", "results": [{"test": "tests/x.rs > manual_metric", "status": "pass"}]}),
    )
    .await?;

    let conn = w1.pool.get().await?;
    let empty_commits: i64 = conn
        .query_one(
            "SELECT count(*) FROM joppa.test_reports WHERE workspace=$1 AND commit_sha=''",
            &[&w1.ws],
        )
        .await?
        .get(0);
    ensure!(empty_commits >= 2, "expected empty commit rows, got {empty_commits}");

    let bad_w1 = w1
        .mcp(
            &w1.bot,
            "joppa_record_results",
            json!({"commit": "ABC", "results": [{"test": "tests/x.rs > manual_metric", "status": "pass"}]}),
        )
        .await?;
    ensure!(
        bad_w1 == Err("commit must be a lowercase git sha".into()),
        "{bad_w1:?}"
    );

    let bad_w2 = w2
        .mcp(
            &w2.bot,
            "joppa_record_results",
            json!({"results": [{"test": "tests/x.rs > code_test", "status": "pass"}]}),
        )
        .await?;
    ensure!(
        bad_w2 == Err("commit must be a lowercase git sha".into()),
        "{bad_w2:?}"
    );
    Ok(())
}

// ac:szu8fkbc
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn contract_only_reset_keeps_metadata_and_equal_source_and_honors_type_flag() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > contract_only"]).await?;
    let tree = ok(&w, "joppa_tree", json!({})).await?;
    w.command(
        &w.owner,
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Configure source, stage on ac, and resetting env on note",
            "put_types": {
                "source": {"title": "Source", "role": "document", "status": "none", "root": true, "source": true, "parents": []},
                "note": {"title": "Note", "role": "document", "status": "none", "root": true, "parents": [], "fields": {"env": {"type": "text", "reset_on_change": true}}},
                "ac": {
                    "title": "Criteria",
                    "role": "ac",
                    "status": "children",
                    "parents": ["requirement"],
                    "fields": {"stage": {"type": "enum", "values": ["todo", "done"]}},
                    "references": {"sources": {"target": "source", "multiple": true, "required": false, "title": "Sources"}}
                }
            }
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    for id in ["s1", "s2"] {
        w.command(
            &w.bot,
            json!({"op": "save_object", "kind": "source", "id": id, "revision": 0, "parent": null, "data": {"title": "Decision", "owner": "owner", "body": "Keep fee at 2%"}, "reason": "Seed source"}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
        ok(
            &w,
            "joppa_attach",
            json!({"target": format!("source:{id}"), "filename": "spec.txt", "content": "Fee=2%"}),
        )
        .await?;
    }

    w.command(
        &w.bot,
        json!({"op": "save_object", "kind": "ac", "id": "a", "revision": 1, "parent": "requirement:r", "data": {"title": "A", "owner": "owner", "body": "Fee is 2%", "stage": "todo", "sources": ["source:s1"]}, "reason": "Set contract"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    w.recorded(
        &"b".repeat(40),
        json!([{"test": "tests/x.rs > contract_only", "status": "pass"}]),
    )
    .await?;
    ensure!(w.object("ac:a").await?["status"]["tone"] == "green");

    // Changing owner, assignee, stage, and swapping s1 -> equal s2 keeps ac:a green.
    w.command(
        &w.bot,
        json!({"op": "save_object", "kind": "ac", "id": "a", "revision": 2, "parent": "requirement:r", "data": {"owner": "alex", "assignee": "crew/p1", "stage": "done", "sources": ["source:s2"]}, "reason": "Metadata and dedup source"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    ensure!(
        w.object("ac:a").await?["status"]["tone"] == "green",
        "metadata or equal source swap reset green test"
    );

    // Changing body resets ac:a to blue.
    w.command(
        &w.bot,
        json!({"op": "save_object", "kind": "ac", "id": "a", "revision": 3, "parent": "requirement:r", "data": {"body": "Fee is 3%"}, "reason": "Contract change"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    ensure!(w.object("ac:a").await?["status"]["tone"] == "blue");

    // Setting reset_on_change false on type ac makes earlier records count again.
    let tree = ok(&w, "joppa_tree", json!({})).await?;
    let mut ac_type = tree["schema"]["types"]["ac"].clone();
    ac_type["reset_on_change"] = json!(false);
    w.command(
        &w.owner,
        json!({"op": "set_tree_schema", "revision": tree["schema"]["revision"], "put_types": {"ac": ac_type}, "reason": "Disable reset on ac type"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    ensure!(w.object("ac:a").await?["status"]["tone"] == "green");

    // Setting reset_on_change on a non-ac role is refused.
    let tree = ok(&w, "joppa_tree", json!({})).await?;
    let mut req_type = tree["schema"]["types"]["requirement"].clone();
    req_type["reset_on_change"] = json!(false);
    let refused = w
        .command(
            &w.owner,
            json!({"op": "set_tree_schema", "revision": tree["schema"]["revision"], "put_types": {"requirement": req_type}, "reason": "Invalid role"}),
        )
        .await?;
    ensure!(
        refused == Err("reset_on_change of a type applies to the role ac".into()),
        "{refused:?}"
    );
    Ok(())
}

// ac:l6drwbo5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn list_filters_not_data_has_tests_and_comment_prefix_match_exact_sets() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let tree = ok(&w, "joppa_tree", json!({})).await?;
    let mut ac_type = tree["schema"]["types"]["ac"].clone();
    ac_type["fields"] = json!({"stage": {"type": "enum", "values": ["to_decide", "done"]}});
    w.command(
        &w.owner,
        json!({"op": "set_tree_schema", "revision": tree["schema"]["revision"], "put_types": {"ac": ac_type}, "reason": "Add stage on ac"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    let ops = json!({
        "request_id": "seed-filter-acs",
        "operations": [
            {"command": {"op": "save_object", "kind": "ac", "id": "a", "revision": 1, "parent": "requirement:r", "data": {"title": "A", "owner": "owner", "stage": "done"}, "reason": "Update a"}},
            {"command": {"op": "save_object", "kind": "test", "id": "t1", "revision": 0, "parent": "ac:a", "data": {"title": "T1", "owner": "owner"}, "reason": "Add t1"}},
            {"command": {"op": "save_object", "kind": "test", "id": "t2", "revision": 0, "parent": "ac:a", "data": {"title": "T2", "owner": "owner"}, "reason": "Add t2"}},
            {"command": {"op": "save_object", "kind": "ac", "id": "b", "revision": 0, "parent": "requirement:r", "data": {"title": "B", "owner": "owner", "stage": "to_decide"}, "reason": "Add b"}},
            {"command": {"op": "save_object", "kind": "ac", "id": "c", "revision": 0, "parent": "requirement:r", "data": {"title": "C", "owner": "owner"}, "reason": "Add c"}},
            {"command": {"op": "note", "target": "ac:c", "id": "p1-done-x", "text": "Active gap"}},
            {"command": {"op": "note", "target": "ac:c", "id": "p2-hold-y", "text": "Withdrawn gap"}},
            {"command": {"op": "withdraw_note", "target": "ac:c", "id": "p2-hold-y", "reason": "Obsolete"}}
        ]
    });
    ok(&w, "joppa_batch", ops).await?;

    let addrs = |reply: Value| -> Vec<String> {
        reply["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i["address"].as_str().map(str::to_owned))
            .collect()
    };

    let not_done = ok(
        &w,
        "joppa_objects",
        json!({"kind": "ac", "not_data": {"stage": "done"}}),
    )
    .await?;
    ensure!(addrs(not_done) == vec!["ac:b", "ac:c"]);

    let with_tests = ok(
        &w,
        "joppa_objects",
        json!({"kind": "ac", "has_tests": true}),
    )
    .await?;
    ensure!(addrs(with_tests) == vec!["ac:a"]);

    let without_tests = ok(
        &w,
        "joppa_objects",
        json!({"kind": "ac", "has_tests": false}),
    )
    .await?;
    ensure!(addrs(without_tests) == vec!["ac:b", "ac:c"]);

    let p1 = ok(
        &w,
        "joppa_objects",
        json!({"kind": "ac", "comment_prefix": "p1-"}),
    )
    .await?;
    ensure!(addrs(p1) == vec!["ac:c"]);

    let p2 = ok(
        &w,
        "joppa_objects",
        json!({"kind": "ac", "comment_prefix": "p2-"}),
    )
    .await?;
    ensure!(addrs(p2).is_empty());

    let counted = ok(
        &w,
        "joppa_objects",
        json!({"kind": "ac", "count": true, "not_data": {"stage": "done"}}),
    )
    .await?;
    ensure!(counted == json!({"total": 2}), "{counted}");
    Ok(())
}

// ac:pquuuvss
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn history_filters_by_alias_op_kind_and_target_include_aliases() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let tree = ok(&w, "joppa_tree", json!({})).await?;
    w.command(
        &w.owner,
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Add domain and capability",
            "put_types": {
                "domain": {"title": "Domain", "role": "group", "status": "children", "root": true, "parents": []},
                "capability": {"title": "Capability", "role": "group", "status": "children", "parents": ["domain"]}
            }
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    ok(
        &w,
        "joppa_batch",
        json!({
            "alias": "crew/p1",
            "request_id": "hist-p1-saves",
            "operations": [
                {"command": {"op": "save_object", "kind": "domain", "id": "d", "revision": 0, "parent": null, "data": {"title": "D", "owner": "owner"}, "reason": "Create d"}},
                {"command": {"op": "save_object", "kind": "capability", "id": "c", "revision": 0, "parent": "domain:d", "data": {"title": "C", "owner": "owner"}, "reason": "Create c"}}
            ]
        }),
    )
    .await?;
    ok(
        &w,
        "joppa_command",
        json!({"alias": "p2", "request_id": "hist-p2-note", "command": {"op": "note", "target": "domain:d", "id": "nd1", "text": "Domain note"}}),
    )
    .await?;

    w.pool
        .get()
        .await?
        .execute(
            "INSERT INTO joppa.object_aliases(workspace, address, target) VALUES ($1, 'domain:old', 'domain:d')",
            &[&w.ws],
        )
        .await?;

    let by_alias = ok(&w, "joppa_history", json!({"by_alias": "crew/p1"})).await?;
    ensure!(
        by_alias["total"] == 2 && by_alias["items"].as_array().map(Vec::len) == Some(2),
        "{by_alias}"
    );

    let by_op = ok(&w, "joppa_history", json!({"op": "note"})).await?;
    ensure!(
        by_op["total"] == 1 && by_op["items"][0]["address"] == "domain:d",
        "{by_op}"
    );

    let by_kind = ok(&w, "joppa_history", json!({"kind": "capability"})).await?;
    ensure!(
        by_kind["total"] == 1 && by_kind["items"][0]["address"] == "capability:c",
        "{by_kind}"
    );

    let by_target = ok(&w, "joppa_history", json!({"target": "domain:d"})).await?;
    let by_old = ok(&w, "joppa_history", json!({"target": "domain:old"})).await?;
    ensure!(
        by_target["total"] == 2 && by_target["items"] == by_old["items"] && by_old["total"] == 2,
        "{by_target} vs {by_old}"
    );
    Ok(())
}
