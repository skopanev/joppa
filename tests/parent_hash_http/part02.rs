// Part 2 of tests/parent_hash_http.rs: ac:fug439p5 and ac:1pbb9gtx.

// ac:fug439p5 (off_nominal)
// test:602x0ya8
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn save_object_refuses_wrong_parent_hash_computed_fields_and_write_key_stamps() -> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_domain_capability(&w, true).await?;

    ok(&w, json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,
        "data":{"title":"D","owner":"owner","body":"v1"},"reason":"Create D"})).await?;
    ok(&w, json!({"op":"save_object","kind":"domain","id":"e","revision":0,"parent":null,
        "data":{"title":"E","owner":"owner","body":"root"},"reason":"Create E"})).await?;
    ok(&w, json!({"op":"save_object","kind":"capability","id":"c","revision":0,"parent":"domain:d",
        "data":{"title":"C","owner":"owner","body":"cap"},"reason":"Create C"})).await?;
    let old_stamp = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"capability:c"}))
        .await?["parent_hash"]
        .clone();
    ok(&w, json!({"op":"save_object","kind":"domain","id":"d","revision":1,
        "data":{"body":"v2"},"reason":"Edit D"})).await?;
    let cur_stamp = mcp_ok(
        &w,
        &w.bot,
        "joppa_objects",
        json!({"address":"capability:c","fields":["parent_hash_current"]}),
    )
    .await?["parent_hash_current"]
    .clone();

    let jpt = create_write_key(&w).await?;
    let pos = head(&w).await?;

    for bad in [json!("abc"), Value::Null, json!(7), old_stamp] {
        let r = save(
            &w,
            json!({"op":"save_object","kind":"capability","id":"c","revision":1,
                "data":{"parent_hash":bad},"reason":"Bad confirm"}),
        )
        .await?;
        refused(r, "parent_hash does not match the current parent contract")?;
    }
    let root_bad = save(
        &w,
        json!({"op":"save_object","kind":"domain","id":"e","revision":1,
            "data":{"parent_hash":cur_stamp},"reason":"Root confirm"}),
    )
    .await?;
    refused(root_bad, "parent_hash does not match the current parent contract")?;

    for (field, msg) in [
        ("parent_hash_current", "parent_hash_current is computed by the server"),
        ("parent_changed", "parent_changed is computed by the server"),
    ] {
        let r = save(
            &w,
            json!({"op":"save_object","kind":"capability","id":"c","revision":1,
                "data":{field:"x"},"reason":"Computed field"}),
        )
        .await?;
        refused(r, msg)?;
    }

    let wk_reply = w
        .command(
            &jpt,
            json!({"op":"save_object","kind":"capability","id":"c","revision":1,
                "data":{"stage":"done","parent_hash":cur_stamp},"reason":"Write key confirm"}),
        )
        .await?;
    refused(wk_reply, "does not allow key_write")?;
    ensure!(head(&w).await? == pos, "a refused save appended a journal event");
    Ok(())
}

// ac:fug439p5 (boundary)
// test:parflthe
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn set_tree_schema_refuses_typed_and_reference_fields_using_reserved_parent_hash_names()
-> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_domain_capability(&w, false).await?;
    let tree = mcp_ok(&w, &w.bot, "joppa_tree", json!({})).await?;
    let cap_type = tree["schema"]["types"]["capability"].clone();
    let rev = tree["schema"]["revision"].clone();
    let pos = head(&w).await?;

    for name in ["parent_hash", "parent_hash_current", "parent_changed"] {
        let mut t_field = cap_type.clone();
        t_field["fields"] = json!({name: {"type":"text"}});
        let r1 = w
            .command(
                &w.owner,
                json!({"op":"set_tree_schema","revision":rev,"put_types":{"capability":t_field},"reason":"Bad field"}),
            )
            .await?;
        refused(r1, &format!("field {name} is computed by the server"))?;

        let mut t_ref = cap_type.clone();
        t_ref["references"] = json!({name: {"target":"domain","title":"Ref","multiple":false,"required":false}});
        let r2 = w
            .command(
                &w.owner,
                json!({"op":"set_tree_schema","revision":rev,"put_types":{"capability":t_ref},"reason":"Bad ref"}),
            )
            .await?;
        refused(r2, &format!("field {name} is computed by the server"))?;
    }
    ensure!(head(&w).await? == pos, "a refused schema appended a journal event");
    Ok(())
}

// ac:1pbb9gtx (nominal)
// test:vuhxuf4t
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn journal_replay_preserves_epoch_stamps_and_restore_stamps_the_live_parent() -> Result<()> {
    let mut w = record_world::world(&[]).await?;
    setup_domain_capability(&w, false).await?;
    ok(&w, json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,
        "data":{"title":"D","owner":"owner","body":"v1"},"reason":"Create D"})).await?;
    w.stop();
    let pre_pos = head(&w).await? + 1;
    let epoch = pre_pos + 1;
    let conn = w.pool.get().await?;
    conn.execute(
        "UPDATE joppa.replay_epochs SET first_position=$2 WHERE workspace=$1 AND rule='parent_hash'",
        &[&w.ws, &epoch],
    )
    .await?;
    conn.execute("DELETE FROM joppa.object_revisions WHERE workspace=$1", &[&w.ws]).await?;
    conn.execute("DELETE FROM joppa.objects WHERE workspace=$1", &[&w.ws]).await?;
    conn.execute("UPDATE joppa.board_cache SET first_view='{}'::jsonb WHERE workspace=$1", &[&w.ws]).await?;
    drop(conn);
    save_world::append_event(
        &w,
        "bot",
        &json!({"op":"save_object","kind":"capability","id":"pre","revision":0,"parent":"domain:d",
            "data":{"title":"Pre","owner":"owner","body":"pre"},"relations":{},"reason":"Pre-epoch child"}),
    )
    .await?;
    w.start().await?;

    ok(&w, json!({"op":"save_object","kind":"capability","id":"post","revision":0,"parent":"domain:d",
        "data":{"title":"Post","owner":"owner","body":"post"},"reason":"Post-epoch child"})).await?;
    ok(&w, json!({"op":"save_object","kind":"capability","id":"k","revision":0,"parent":"domain:d",
        "data":{"title":"K","owner":"owner","body":"k"},"reason":"Create K"})).await?;
    ok(&w, json!({"op":"archive_object","target":"capability:k","revision":1,"reason":"Archive K"})).await?;

    let post_before = revisions(&w, "capability:post").await?;
    let pre_before = revisions(&w, "capability:pre").await?;
    ensure!(pre_before[0].1.get("parent_hash").is_none() && is_sha256_hex(&post_before[0].1["parent_hash"]));

    replay(&w).await?;
    ensure!(revisions(&w, "capability:pre").await? == pre_before && revisions(&w, "capability:post").await? == post_before);

    ok(&w, json!({"op":"save_object","kind":"domain","id":"d","revision":1,
        "data":{"body":"v2"},"reason":"Update D before restore"})).await?;
    ok(&w, json!({"op":"restore_object","target":"capability:k","revision":2,"reason":"Restore K"})).await?;
    let k_read = mcp_ok(
        &w,
        &w.bot,
        "joppa_objects",
        json!({"address":"capability:k","fields":["revision","parent_hash","parent_hash_current","parent_changed"]}),
    )
    .await?;
    ensure!(
        k_read["revision"] == 3
            && is_sha256_hex(&k_read["parent_hash"])
            && k_read["parent_hash"] != post_before[0].1["parent_hash"]
            && k_read["parent_hash"] == k_read["parent_hash_current"]
            && k_read["parent_changed"] == false,
        "restored K: {k_read}"
    );
    Ok(())
}

// ac:1pbb9gtx (boundary)
// test:slte7uc6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn catch_up_parent_hashes_fills_missing_columns_without_journal_replay_and_keeps_version_mark()
-> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_domain_capability(&w, false).await?;
    ok(&w, json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,
        "data":{"title":"D","owner":"owner","body":"v1"},"reason":"Create D"})).await?;
    ok(&w, json!({"op":"save_object","kind":"capability","id":"c","revision":0,"parent":"domain:d",
        "data":{"title":"C","owner":"owner","body":"c"},"reason":"Create C"})).await?;
    let expected = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"capability:c"}))
        .await?["parent_hash"]
        .as_str()
        .context("stamp")?
        .to_owned();

    let conn = w.pool.get().await?;
    conn.execute("UPDATE joppa.objects SET parent_hash_current=NULL WHERE workspace=$1", &[&w.ws]).await?;
    conn.execute("UPDATE joppa.board_cache SET first_view=first_view-'parent_hash_version' WHERE workspace=$1", &[&w.ws]).await?;
    let pos = head(&w).await?;
    let caught = joppa::store::catch_up_parent_hashes(&w.pool).await?;
    ensure!(caught >= 1 && head(&w).await? == pos, "catch_up: {caught}");

    let row = conn
        .query_one(
            "SELECT (SELECT parent_hash_current FROM joppa.objects WHERE workspace=$1 AND address='capability:c'),
                    first_view->>'parent_hash_version', first_view->>'projection_version'
             FROM joppa.board_cache WHERE workspace=$1",
            &[&w.ws],
        )
        .await?;
    ensure!(
        row.get::<_, Option<String>>(0).as_deref() == Some(expected.as_str())
            && row.get::<_, Option<String>>(1).as_deref() == Some("2")
            && row.get::<_, Option<String>>(2).as_deref() == Some("22")
    );
    ok(&w, json!({"op":"save_object","kind":"domain","id":"e","revision":0,"parent":null,
        "data":{"title":"E","owner":"owner","body":"e"},"reason":"Later write"})).await?;
    let after: Option<String> = conn
        .query_one("SELECT first_view->>'parent_hash_version' FROM joppa.board_cache WHERE workspace=$1", &[&w.ws])
        .await?
        .get(0);
    ensure!(after.as_deref() == Some("2"), "after write: {after:?}");
    Ok(())
}
