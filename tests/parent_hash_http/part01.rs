// Part 1 of tests/parent_hash_http.rs: ac:dvur104e and ac:ipdef4dm.

// ac:dvur104e
// test:y2jw43pz
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn save_move_and_confirmation_stamp_parent_hash_while_quiet_and_empty_saves_keep_it()
-> Result<()> {
    let w = record_world::world(&[]).await?;
    setup_domain_capability(&w, false).await?;
    ok(&w, json!({"op":"save_object","kind":"domain","id":"d","revision":0,"parent":null,"data":{"title":"Domain D","owner":"owner","body":"a"},"reason":"Create D"})).await?;
    ok(&w, json!({"op":"save_object","kind":"domain","id":"e","revision":0,"parent":null,"data":{"title":"Domain E","owner":"owner","body":"e"},"reason":"Create E"})).await?;
    ok(&w, json!({"op":"save_object","kind":"capability","id":"c","revision":0,"parent":"domain:d","data":{"title":"Cap C","owner":"owner","body":"cap"},"reason":"Create C"})).await?;

    let fields = json!(["parent_hash", "parent_hash_current", "parent_changed", "revision"]);
    let r1 = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"capability:c","fields":fields})).await?;
    ensure!(
        is_sha256_hex(&r1["parent_hash"])
            && r1["parent_hash"] == r1["parent_hash_current"]
            && r1["parent_changed"] == false
            && r1["revision"] == 1,
        "initial read: {r1}"
    );
    let h_d_a = r1["parent_hash"].clone();

    // Changing only owner on D keeps the contract hash of D.
    ok(&w, json!({"op":"save_object","kind":"domain","id":"d","revision":1,"data":{"owner":"alex"},"reason":"Owner change on D"})).await?;
    let after_owner_d = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"capability:c","fields":fields})).await?;
    ensure!(after_owner_d["parent_hash_current"] == h_d_a && after_owner_d["parent_changed"] == false, "after D owner change: {after_owner_d}");

    // Changing body of D to "b" updates parent_hash_current and sets parent_changed true.
    ok(&w, json!({"op":"save_object","kind":"domain","id":"d","revision":2,"data":{"body":"b"},"reason":"Body change on D"})).await?;
    let after_body_d = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"capability:c","fields":fields})).await?;
    let h_d_b = after_body_d["parent_hash_current"].clone();
    ensure!(is_sha256_hex(&h_d_b) && h_d_b != h_d_a && after_body_d["parent_hash"] == h_d_a && after_body_d["parent_changed"] == true, "after D body change: {after_body_d}");

    // Saving only owner and assignee on C keeps the old stamp and parent_changed true.
    ok(&w, json!({"op":"save_object","kind":"capability","id":"c","revision":1,"data":{"owner":"alex","assignee":"lane1"},"reason":"Quiet save on C"})).await?;
    let after_quiet_c = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"capability:c","fields":fields})).await?;
    ensure!(after_quiet_c["revision"] == 2 && after_quiet_c["parent_hash"] == h_d_a && after_quiet_c["parent_changed"] == true, "after quiet save on C: {after_quiet_c}");

    // An empty save of C adds no revision and no journal event, and keeps parent_changed true.
    let pos_before_empty = head(&w).await?;
    ok(&w, json!({"op":"save_object","kind":"capability","id":"c","revision":2,"reason":"Guard"})).await?;
    ensure!(head(&w).await? == pos_before_empty, "empty save wrote an event");
    let after_empty_c = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"capability:c","fields":fields})).await?;
    ensure!(after_empty_c["revision"] == 2 && after_empty_c["parent_changed"] == true, "after empty save: {after_empty_c}");

    // Confirmation with data.parent_hash == parent_hash_current stamps h_d_b without storing parent_hash in data.
    ok(
        &w,
        json!({"op":"save_object","kind":"capability","id":"c","revision":2,
            "data":{"parent_hash":h_d_b},"reason":"Confirm C against D"}),
    )
    .await?;
    let card_c = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"capability:c"})).await?;
    ensure!(
        card_c["revision"] == 3
            && card_c["parent_hash"] == h_d_b
            && card_c.get("parent_changed").is_none()
            && card_c["data"].get("parent_hash").is_none(),
        "confirmed card: {card_c}"
    );
    // Repeating the same confirmation is a no-op.
    let pos_before_repeat = head(&w).await?;
    ok(
        &w,
        json!({"op":"save_object","kind":"capability","id":"c","revision":3,
            "data":{"parent_hash":h_d_b},"reason":"Repeat confirm"}),
    )
    .await?;
    ensure!(head(&w).await? == pos_before_repeat, "repeat confirmation wrote an event");

    // Moving C under E stamps the hash of E.
    ok(
        &w,
        json!({"op":"save_object","kind":"capability","id":"c","revision":3,"parent":"domain:e",
            "reason":"Move C under E"}),
    )
    .await?;
    let after_move =
        mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"capability:c","fields":fields})).await?;
    ensure!(
        after_move["revision"] == 4
            && is_sha256_hex(&after_move["parent_hash"])
            && after_move["parent_hash"] != h_d_b
            && after_move["parent_hash"] == after_move["parent_hash_current"]
            && after_move["parent_changed"] == false,
        "after move to E: {after_move}"
    );
    Ok(())
}

// ac:ipdef4dm
// test:6kzfyo5b
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn mcp_and_http_reads_track_source_edits_files_quiet_fields_and_archived_parents()
-> Result<()> {
    let fake = record_world::fake_spaces::start();
    let w = record_world::world_env(&[], &fake.env()).await?;
    let tree = mcp_ok(&w, &w.bot, "joppa_tree", json!({})).await?;
    let mut ac_type = tree["schema"]["types"]["ac"].clone();
    ac_type["fields"] = json!({"stage":{"type":"enum","values":["todo","done"],"reset_on_change":false}});
    ac_type["references"] = json!({"sources":{"target":"source","title":"Sources","multiple":true,"required":false}});
    let source_type = json!({
        "title":"Source","role":"document","root":true,"source":true,
        "status":"none","parents":["source"]
    });
    w.command(
        &w.owner,
        json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],
            "put_types":{"source":source_type,"ac":ac_type},"reason":"Add source and quiet stage"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    ok(&w, json!({"op":"save_object","kind":"source","id":"s1","revision":0,"parent":null,
        "data":{"title":"Spec S1","owner":"owner","body":"v1"},"reason":"Create S1"})).await?;
    ok(&w, json!({"op":"save_object","kind":"ac","id":"s","revision":0,"parent":"requirement:r",
        "data":{"title":"AC S","owner":"owner","body":"Rule","sources":["source:s1"]},
        "reason":"Create AC S"})).await?;
    ok(&w, json!({"op":"save_object","kind":"test","id":"t","revision":0,"parent":"ac:s",
        "data":{"title":"Scenario T","owner":"owner","body":"Check"},"reason":"Create T"})).await?;

    // Plain list gives only address, title, status (no parent_* keys).
    let plain = mcp_ok(&w, &w.bot, "joppa_objects", json!({"parent":"ac:s"})).await?;
    let keys: Vec<&str> = plain["items"][0]
        .as_object()
        .context("obj")?
        .keys()
        .map(String::as_str)
        .collect();
    ensure!(keys == ["address", "status", "title"] || keys.iter().all(|k| ["address","title","status"].contains(k)), "plain list: {plain}");

    // Read without fields shows parent_hash and omits parent_changed and parent_hash_current when false.
    let card = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"test:t"})).await?;
    ensure!(
        is_sha256_hex(&card["parent_hash"])
            && card.get("parent_changed").is_none()
            && card.get("parent_hash_current").is_none(),
        "fresh card: {card}"
    );
    let h1 = card["parent_hash"].clone();

    // Edit S1 body -> T becomes parent_changed true across MCP and HTTP, while AC S stays false.
    ok(&w, json!({"op":"save_object","kind":"source","id":"s1","revision":1,
        "data":{"body":"v2"},"reason":"Edit S1"})).await?;
    let f3 = json!(["parent_hash", "parent_hash_current", "parent_changed"]);
    let mcp_t = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"test:t","fields":f3})).await?;
    let mcp_s = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"ac:s","fields":f3})).await?;
    let http_one = http_get(&w, "address=test:t").await?;
    let http_many = http_get(&w, "address=ac:s,test:t&fields=parent_hash,parent_hash_current,parent_changed").await?;
    let http_list = http_get(&w, "parent=ac:s&fields=parent_hash,parent_hash_current,parent_changed").await?;
    ensure!(
        mcp_t["parent_changed"] == true
            && mcp_s["parent_changed"] == false
            && http_one["parent_hash"] == mcp_t["parent_hash"]
            && http_one["parent_hash_current"] == mcp_t["parent_hash_current"]
            && http_one["parent_changed"] == true
            && http_many["items"][0]["parent_changed"] == false
            && http_many["items"][1]["parent_changed"] == true
            && http_list["items"][0]["parent_changed"] == true,
        "after S1 body edit: {mcp_t} {http_one} {http_many} {http_list}"
    );

    // Confirm T, then attach a file to S1 -> T flips to parent_changed true again.
    let h2 = mcp_t["parent_hash_current"].clone();
    ok(&w, json!({"op":"save_object","kind":"test","id":"t","revision":1,
        "data":{"parent_hash":h2},"reason":"Confirm T v2"})).await?;
    manual_check::check(&w, "source:s1", json!({})).await?;
    let after_file = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"test:t","fields":f3})).await?;
    ensure!(after_file["parent_changed"] == true && after_file["parent_hash_current"] != h2, "after S1 file: {after_file}");

    // Confirm T, then set non-reset typed field stage on AC S -> hash unchanged, parent_changed stays false.
    let h3 = after_file["parent_hash_current"].clone();
    ok(&w, json!({"op":"save_object","kind":"test","id":"t","revision":2,
        "data":{"parent_hash":h3},"reason":"Confirm T v3"})).await?;
    ok(&w, json!({"op":"save_object","kind":"ac","id":"s","revision":1,
        "data":{"stage":"done"},"reason":"Quiet stage on AC S"})).await?;
    let after_stage = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"test:t","fields":f3})).await?;
    ensure!(after_stage["parent_hash_current"] == h3 && after_stage["parent_changed"] == false, "after stage: {after_stage}");

    // Older revision 1 with fields shows old stamp h1 vs current h3 (parent_changed true), while plain revision 1 omits them.
    let rev1_f = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"test:t","revision":1,"fields":f3})).await?;
    let rev1_plain = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"test:t","revision":1})).await?;
    ensure!(
        rev1_f["parent_hash"] == h1 && rev1_f["parent_hash_current"] == h3 && rev1_f["parent_changed"] == true
            && rev1_plain.get("parent_hash").is_none() && rev1_plain.get("parent_changed").is_none(),
        "rev1 reads: {rev1_f} {rev1_plain}"
    );

    // Archive T and then AC S -> archived parent has no current hash, so T shows parent_changed true.
    ok(&w, json!({"op":"archive_object","target":"test:t","revision":3,"reason":"Archive T"})).await?;
    ok(&w, json!({"op":"archive_object","target":"ac:s","revision":2,"reason":"Archive AC S"})).await?;
    let arch_mcp = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"test:t","fields":f3})).await?;
    let arch_http = http_get(&w, "address=test:t").await?;
    ensure!(
        arch_mcp["parent_hash"] == h3
            && arch_mcp["parent_hash_current"].is_null()
            && arch_mcp["parent_changed"] == true
            && arch_http["parent_hash_current"].is_null()
            && arch_http["parent_changed"] == true,
        "after parent archive: {arch_mcp} {arch_http}"
    );
    Ok(())
}
