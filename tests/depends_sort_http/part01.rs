// Part 1 of tests/depends_sort_http.rs: ac:e0i44hbz (test:wjt5v4nq and test:0938ls1q).

// ac:e0i44hbz
// test:wjt5v4nq
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn done_and_green_rules_drive_blocked_across_all_reads() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_dep_schema(&w).await?;
    let cookie = session(&w, "alex").await?;
    let rkey = create_key(&w, &cookie, &w.ws).await?;

    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"requirement","id":"rq","revision":0,"parent":null,"data":{"title":"Req","owner":"owner","body":"Rule"},"reason":"Init"})).await?;
    for id in ["a1", "a2"] {
        cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"ac","id":id,"revision":0,"parent":"requirement:rq","data":{"title":id,"owner":"owner"},"reason":"Init"})).await?;
    }
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"test","id":"chk1","revision":0,"parent":"ac:a1","data":{"title":"chk1","owner":"owner"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"t1","revision":0,"parent":"ac:a2","data":{"title":"T1","owner":"owner","stage":"todo"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"t2","revision":0,"parent":"ac:a2","data":{"title":"T2","owner":"owner","stage":"todo"},"reason":"Init"})).await?;
    // x waits on t1 (status none, stage todo), z is child of x, y waits on ac:a1 (status children, blue/red).
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"x","revision":0,"parent":"ac:a2","data":{"title":"X","owner":"owner","stage":"todo"},"relations":{"depends_on":{"edges":[{"to":"task:t1"}]}},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"subtask","id":"z","revision":0,"parent":"task:x","data":{"title":"Z","owner":"owner","stage":"todo"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"y","revision":0,"parent":"ac:a2","data":{"title":"Y","owner":"owner","stage":"todo"},"relations":{"depends_on":{"edges":[{"to":"ac:a1"}]}},"reason":"Init"})).await?;

    // Record fail on chk1 so ac:a1 is red -> x, z, y are all blocked.
    mcp_ok(&w, &w.bot, "joppa_record_results", json!({"commit":"1111111","results":[{"test":"test:chk1","status":"fail"}]})).await?;

    let mcp_b = mcp_ok(&w, &w.bot, "joppa_objects", json!({"blocked":true})).await?;
    ensure!(item_addrs(&mcp_b) == ["subtask:z", "task:x", "task:y"], "mcp blocked: {mcp_b}");
    let http_b = http_get(&w, "blocked=true").await?;
    ensure!(item_addrs(&http_b) == ["subtask:z", "task:x", "task:y"], "http blocked: {http_b}");
    let cnt = mcp_ok(&w, &w.bot, "joppa_objects", json!({"blocked":true,"count":true})).await?;
    ensure!(cnt["total"] == 3, "count blocked: {cnt}");
    let grp = mcp_ok(&w, &w.bot, "joppa_objects", json!({"blocked":true,"group_by":["kind"]})).await?;
    ensure!(grp["total"] == 3, "group_by blocked: {grp}");
    let api_b = data_read(&w, &rkey, json!({"filter":{"blocked":true},"fields":["title","blocked"]})).await?;
    let mut api_addrs = item_addrs(&api_b);
    api_addrs.sort();
    ensure!(api_addrs == ["subtask:z", "task:x", "task:y"], "api/data blocked: {api_b}");

    // Mark t1 stage=done -> x and z unblock, only y stays blocked.
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"t1","revision":1,"data":{"stage":"done"},"reason":"Done"})).await?;
    let mcp_b2 = mcp_ok(&w, &w.bot, "joppa_objects", json!({"blocked":true})).await?;
    ensure!(item_addrs(&mcp_b2) == ["task:y"], "after t1 done: {mcp_b2}");

    // Point x to t2 (todo) -> x and z block again, then archive t2 -> x and z unblock again!
    let x_rev = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"task:x","fields":["revision"]})).await?["revision"].as_u64().context("rev")?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"x","revision":x_rev,"relations":{"depends_on":{"edges":[{"to":"task:t2"}]}},"reason":"Wait on t2"})).await?;
    ensure!(item_addrs(&mcp_ok(&w, &w.bot, "joppa_objects", json!({"blocked":true})).await?).contains(&"subtask:z".to_owned()), "z blocked on t2");
    cmd_ok(&w, &w.bot, json!({"op":"archive_object","target":"task:t2","revision":1,"reason":"Archive t2"})).await?;
    ensure!(item_addrs(&mcp_ok(&w, &w.bot, "joppa_objects", json!({"blocked":true})).await?) == ["task:y"], "after t2 archived");

    // Turn chk1 green -> ac:a1 turns green -> y unblocks too (0 blocked nodes left).
    mcp_ok(&w, &w.bot, "joppa_record_results", json!({"commit":"2222222","results":[{"test":"test:chk1","status":"pass"}]})).await?;
    let none_left = mcp_ok(&w, &w.bot, "joppa_objects", json!({"blocked":true})).await?;
    ensure!(item_addrs(&none_left).is_empty(), "all unblocked: {none_left}");
    let free_tasks = mcp_ok(&w, &w.bot, "joppa_objects", json!({"kind":"task","blocked":false})).await?;
    ensure!(item_addrs(&free_tasks) == ["task:t1", "task:x", "task:y"], "free tasks: {free_tasks}");
    Ok(())
}

// ac:e0i44hbz
// test:0938ls1q
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn done_rule_refusals_and_archived_ac_still_blocks() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_dep_schema(&w).await?;

    let tree = mcp_ok(&w, &w.alex, "joppa_tree", json!({})).await?;
    let rev = tree["schema"]["revision"].as_u64().context("schema rev")?;
    for bad_done in [
        json!({}),
        json!({"stage": null}),
        json!({"depends_on": "x"}),
        json!({"relations": "x"}),
        json!({"links": "x"}),
        json!({"stage": "nope"}),
    ] {
        let res = w
            .mcp(
                &w.alex,
                "joppa_command",
                json!({
                    "request_id": uuid::Uuid::new_v4().to_string(),
                    "command": {
                        "op": "set_tree_schema",
                        "revision": rev,
                        "reason": "Bad done rule",
                        "put_types": {
                            "task": {
                                "title": "Task",
                                "role": "task",
                                "status": "none",
                                "parents": ["ac"],
                                "done": bad_done,
                                "fields": {"stage": {"type": "enum", "values": ["todo", "done"]}}
                            }
                        }
                    }
                }),
            )
            .await?;
        ensure!(res.is_err(), "expected refusal for {bad_done}, got {res:?}");
    }

    // Archiving a status-bearing AC (status children) does NOT satisfy depends_on, while a missing target does.
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"requirement","id":"rq2","revision":0,"parent":null,"data":{"title":"R2","owner":"owner","body":"Rule"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"ac","id":"arch_ac","revision":0,"parent":"requirement:rq2","data":{"title":"Arch AC","owner":"owner"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"ac","id":"host_ac","revision":0,"parent":"requirement:rq2","data":{"title":"Host AC","owner":"owner"},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"wait_ac","revision":0,"parent":"ac:host_ac","data":{"title":"Wait AC","owner":"owner","stage":"todo"},"relations":{"depends_on":{"edges":[{"to":"ac:arch_ac"}]}},"reason":"Init"})).await?;
    cmd_ok(&w, &w.bot, json!({"op":"save_object","kind":"task","id":"wait_ghost","revision":0,"parent":"ac:host_ac","data":{"title":"Wait Ghost","owner":"owner","stage":"todo"},"reason":"Init"})).await?;

    cmd_ok(&w, &w.bot, json!({"op":"archive_object","target":"ac:arch_ac","revision":1,"reason":"Archive untested AC"})).await?;
    // Insert a dangling link to a missing target on wait_ghost to verify missing targets do not block.
    w.pool
        .get()
        .await?
        .execute(
            "INSERT INTO joppa.object_links(workspace,source,target,relation) VALUES($1,'task:wait_ghost','task:missing','depends_on')",
            &[&w.ws],
        )
        .await?;

    let check = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"task:wait_ac,task:wait_ghost","fields":["blocked","dependents"]})).await?;
    ensure!(
        check["items"][0]["blocked"] == true && check["items"][1]["blocked"] == false,
        "archived AC must still block while missing target must not: {check}"
    );
    Ok(())
}
