// Part 2 of tests/p1_feedback_http.rs: ac:bvkl297c and ac:i6ht2djy.

// ac:bvkl297c
// test:gpc6vunk
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p1_8_record_results_dry_run_binds_without_saving_and_live_run_records() -> Result<()> {
    let w = record_world::world(&["suite::t1", "suite::t2"]).await?;
    let counts = || async {
        let row = w
            .pool
            .get()
            .await?
            .query_one(
                "SELECT (SELECT count(*) FROM joppa.test_reports WHERE workspace=$1),
                        (SELECT count(*) FROM joppa.test_results tr JOIN joppa.test_reports r ON r.id=tr.report_id WHERE r.workspace=$1),
                        (SELECT count(*) FROM joppa.test_report_receipts rc JOIN joppa.test_reports r ON r.id=rc.report_id WHERE r.workspace=$1)",
                &[&w.ws],
            )
            .await?;
        anyhow::Ok((row.get::<_, i64>(0), row.get::<_, i64>(1), row.get::<_, i64>(2)))
    };
    let before_pos = head(&w).await?;
    let before_counts = counts().await?;

    let dry = mcp_ok(
        &w,
        &w.bot,
        "joppa_record_results",
        json!({
            "commit": "abc1234",
            "dry_run": true,
            "results": [
                {"test":"suite::t1","status":"pass"},
                {"test":"suite::t2","status":"fail"},
                {"test":"suite::missing_x","status":"pass"}
            ]
        }),
    )
    .await?;
    ensure!(
        dry == json!({"dry_run": true, "commit": "abc1234", "tests": 3, "bound": 2, "unbound": ["suite::missing_x"]}),
        "dry reply: {dry}"
    );
    ensure!(
        head(&w).await? == before_pos && counts().await? == before_counts,
        "dry_run mutated database"
    );
    let ac_before = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"ac:a"})).await?;
    ensure!(ac_before["status"] != "green" && ac_before["status"] != "red", "{ac_before}");

    // Noticesuite::t2 failed in dry_run on commit abc1234, yet a live pass on abc1234 turns green!
    let live = mcp_ok(
        &w,
        &w.bot,
        "joppa_record_results",
        json!({
            "commit": "abc1234",
            "results": [
                {"test":"suite::t1","status":"pass"},
                {"test":"suite::t2","status":"pass"}
            ]
        }),
    )
    .await?;
    ensure!(live["unbound"] == json!([]), "{live}");
    let ac_after = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"ac:a"})).await?;
    ensure!(ac_after["status"]["tone"] == "green", "{ac_after}");
    Ok(())
}

// ac:bvkl297c
// test:jip6ig5f
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p1_8_record_results_dry_run_refuses_viewer_and_invalid_status() -> Result<()> {
    let w = record_world::world_env(&["suite::t1"], &[]).await?;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let viewer = format!("viewer-{suffix}");
    let email = format!("{viewer}@example.com");
    w.pool
        .get()
        .await?
        .execute(
            "INSERT INTO joppa.google_identities(actor,google_sub,email,display_name) VALUES($1,$1,$2,'Viewer')",
            &[&viewer, &email],
        )
        .await?;
    store::grant_email(&w.pool, &w.ws, "owner", &email).await?;
    store::enroll_granted(&w.pool, &viewer, None, None).await?;
    let viewer_tok = oauth_token_for(&w, &viewer).await?;
    let pos = head(&w).await?;

    let v_res = w
        .mcp(
            &viewer_tok,
            "joppa_record_results",
            json!({"commit":"abc1234","dry_run":true,"results":[{"test":"suite::t1","status":"pass"}]}),
        )
        .await?;
    refused(v_res, "Read-only")?;

    let bad_res = w
        .mcp(
            &w.bot,
            "joppa_record_results",
            json!({"commit":"abc1234","dry_run":true,"results":[{"test":"suite::t1","status":"bogus_status"}]}),
        )
        .await?;
    ensure!(bad_res.is_err(), "expected invalid status error: {bad_res:?}");
    ensure!(head(&w).await? == pos, "refused dry_run advanced journal");
    Ok(())
}

// ac:i6ht2djy
// test:bgyiexmq
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn p1_9_derived_fields_match_across_mcp_and_http_and_stay_off_plain_card() -> Result<()> {
    let w = record_world::world(&["suite::t1", "suite::t2"]).await?;
    let tree = mcp_ok(&w, &w.bot, "joppa_tree", json!({})).await?;
    w.command(
        &w.owner,
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "put_types": {
                "requirement": {
                    "title":"Requirements","role":"requirement","status":"children","root":true,"parents":[],
                    "fields":{"tag":{"type":"enum","values":["alpha","beta"]}}
                },
                "ac": {
                    "title":"Criteria","role":"ac","status":"children","parents":["requirement"],
                    "fields":{
                        "stage":{"type":"enum","values":["draft","done"]},
                        "tag":{"type":"enum","values":["alpha","beta"],"from":"parent"}
                    }
                }
            },
            "reason": "Add typed and computed fields"
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    ok(
        &w,
        json!({"op":"save_object","kind":"requirement","id":"r","revision":1,"data":{"tag":"alpha"},"reason":"Set tag"}),
    )
    .await?;
    let long_body = format!("Given:  line one\n\nWhen: line two {}", "z".repeat(500));
    let ac_rev = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"ac:a","fields":["revision"]})).await?["revision"].clone();
    ok(
        &w,
        json!({"op":"save_object","kind":"ac","id":"a","revision":ac_rev,"data":{"body":long_body,"stage":"done"},"reason":"Set body and stage"}),
    )
    .await?;
    mcp_ok(
        &w,
        &w.bot,
        "joppa_record_results",
        json!({"commit":"def5678","results":[{"test":"suite::t1","status":"pass"},{"test":"suite::t2","status":"pass"}]}),
    )
    .await?;

    let plain = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"ac:a"})).await?;
    ensure!(
        plain.get("excerpt").is_none()
            && plain.get("data_keys").is_none()
            && plain.get("status_label").is_none(),
        "plain card leaked derived fields: {plain}"
    );

    let f = json!(["excerpt", "data_keys", "status_label"]);
    let mcp_one = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"ac:a","fields":f})).await?;
    ensure!(
        mcp_one["excerpt"].as_str().map(|s| s.chars().count()) == Some(200)
            && mcp_one["excerpt"]
                .as_str()
                .is_some_and(|s| s.starts_with("Given: line one When: line two z"))
            && mcp_one["data_keys"]
                == json!(["before_launch", "body", "depends_on", "owner", "stage", "tag", "title"])
            && mcp_one["status_label"] == "Children checks passed",
        "mcp_one: {mcp_one}"
    );

    let mut expected_list = mcp_one.clone();
    expected_list["address"] = json!("ac:a");
    let mcp_list = mcp_ok(&w, &w.bot, "joppa_objects", json!({"parent":"requirement:r","fields":f})).await?;
    ensure!(mcp_list["items"][0] == expected_list, "mcp_list: {mcp_list}");
    let mcp_many = mcp_ok(&w, &w.bot, "joppa_objects", json!({"address":"ac:a,requirement:r","fields":f})).await?;
    ensure!(mcp_many["items"][0] == expected_list, "mcp_many: {mcp_many}");

    let http_list = http_get(&w, "parent=requirement:r&fields=excerpt,data_keys,status_label").await?;
    ensure!(http_list["items"][0] == expected_list, "http_list: {http_list}");
    let http_many = http_get(&w, "address=ac:a,requirement:r&fields=excerpt,data_keys,status_label").await?;
    ensure!(http_many["items"][0] == expected_list, "http_many: {http_many}");
    let http_one = http_get(&w, "address=ac:a&fields=excerpt,data_keys,status_label").await?;
    ensure!(
        http_one["excerpt"] == mcp_one["excerpt"]
            && http_one["data_keys"] == mcp_one["data_keys"]
            && http_one["status_label"] == mcp_one["status_label"],
        "http_one: {http_one}"
    );
    Ok(())
}
