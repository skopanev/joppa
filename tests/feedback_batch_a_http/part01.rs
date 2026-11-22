// Part 1 of tests/feedback_batch_a_http.rs: ac:d6n5bm15, ac:zl90a6on, ac:m6goiyaq, ac:bd6w8s1v.

// ac:d6n5bm15
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn list_and_relation_reads_give_comments_files_and_references() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let tree = ok(&w, "joppa_tree", json!({})).await?;
    let rev = tree["schema"]["revision"].clone();
    let schema = json!({
        "op": "set_tree_schema",
        "revision": rev,
        "reason": "Configure domain, capability, source and flow relation",
        "put_relations": {
            "flow": {"title": "Flow", "description": "Flow between capabilities"}
        },
        "put_types": {
            "source": {"title": "Source", "role": "document", "status": "none", "root": true, "source": true, "parents": []},
            "domain": {"title": "Domain", "role": "group", "status": "children", "root": true, "parents": []},
            "capability": {
                "title": "Capability",
                "role": "group",
                "status": "children",
                "parents": ["domain"],
                "references": {"sources": {"target": "source", "multiple": true, "required": false, "title": "Sources"}}
            },
            "requirement": {"title": "Requirement", "role": "requirement", "status": "children", "root": true, "parents": ["capability"]}
        }
    });
    w.command(&w.owner, schema)
        .await?
        .map_err(anyhow::Error::msg)?;

    let setup = json!({
        "request_id": "batch-a-list-fields",
        "operations": [
            {"command": {"op": "save_object", "kind": "source", "id": "s", "revision": 0, "parent": null, "data": {"title": "Spec S", "owner": "owner", "body": "Ground"}, "reason": "Add source"}},
            {"command": {"op": "save_object", "kind": "domain", "id": "d", "revision": 0, "parent": null, "data": {"title": "D", "owner": "owner"}, "reason": "Add domain"}},
            {"command": {"op": "save_object", "kind": "capability", "id": "c2", "revision": 0, "parent": "domain:d", "data": {"title": "C2", "owner": "owner"}, "reason": "Add c2"}},
            {"command": {"op": "save_object", "kind": "capability", "id": "c3", "revision": 0, "parent": "domain:d", "data": {"title": "C3", "owner": "owner"}, "reason": "Add c3"}},
            {"command": {"op": "save_object", "kind": "capability", "id": "c1", "revision": 0, "parent": "domain:d", "data": {"title": "C1", "owner": "owner", "sources": ["source:s"]}, "relations": {"flow": {"edges": [{"to": "capability:c2"}]}}, "reason": "Add c1"}},
            {"command": {"op": "note", "target": "capability:c1", "id": "n1", "text": "First note"}},
            {"command": {"op": "note", "target": "capability:c1", "id": "n2", "text": "Second note"}},
            {"command": {"op": "note", "target": "capability:c1", "id": "n3", "text": "Draft note"}},
            {"command": {"op": "withdraw_note", "target": "capability:c1", "id": "n3", "reason": "Duplicate"}}
        ]
    });
    ok(&w, "joppa_batch", setup).await?;
    ok(
        &w,
        "joppa_attach",
        json!({"target": "capability:c1", "filename": "ev.txt", "content": "Proof"}),
    )
    .await?;

    let fields = json!(["title", "comments", "comments_count", "files", "files_count", "references"]);
    let mut card_c1 = ok(
        &w,
        "joppa_objects",
        json!({"address": "capability:c1", "fields": fields}),
    )
    .await?;
    card_c1["address"] = json!("capability:c1");
    ensure!(
        card_c1["comments_count"] == 2
            && card_c1["files_count"] == 1
            && card_c1["files"].as_array().map(Vec::len) == Some(1)
            && card_c1["references"][0]["target"] == "source:s",
        "{card_c1}"
    );

    let list = ok(
        &w,
        "joppa_objects",
        json!({"parent": "domain:d", "fields": fields}),
    )
    .await?;
    let items = list["items"].as_array().context("list items")?;
    ensure!(items.len() == 3 && items[0] == card_c1, "{list}");
    for empty in [&items[1], &items[2]] {
        ensure!(
            empty["comments"].is_null()
                && empty["comments_count"] == 0
                && empty["files"].is_null()
                && empty["files_count"] == 0
                && empty["references"].is_null(),
            "{empty}"
        );
    }

    let flow = ok(
        &w,
        "joppa_objects",
        json!({"in_relation": "flow", "fields": fields}),
    )
    .await?;
    let flow_items = flow["items"].as_array().context("flow items")?;
    ensure!(
        flow_items.len() == 2 && flow_items[0] == card_c1 && flow_items[1] == items[1],
        "{flow}"
    );

    let bad = w
        .mcp(
            &w.bot,
            "joppa_objects",
            json!({"parent": "domain:d", "fields": ["relation_groups"]}),
        )
        .await?;
    ensure!(
        bad == Err("unknown field relation_groups".into()),
        "{bad:?}"
    );
    Ok(())
}

// ac:zl90a6on
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn redaction_masks_comments_intake_and_search_without_reset_or_bump() -> Result<()> {
    let mut w = record_world::world(&["tests/x.rs > redaction_keeps_green"]).await?;
    let secret = "K7Q-555";
    w.command(
        &w.bot,
        json!({"op": "save_object", "kind": "ac", "id": "a", "revision": 1, "parent": "requirement:r", "data": {"title": "A", "owner": "owner", "body": format!("Check token {secret} safely")}, "reason": "Add body"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    w.command(
        &w.bot,
        json!({"op": "note", "target": "ac:a", "id": "na1", "text": format!("Criterion comment {secret}")}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    w.command(
        &w.bot,
        json!({"op": "note", "target": "requirement:r", "id": "nr1", "text": format!("Verification comment {secret}")}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    // Seed a legacy intake state entry holding the secret, then reload the server cache.
    w.stop();
    let conn = w.pool.get().await?;
    let intake = json!({"id": "in1", "said": format!("Customer pasted {secret}"), "author": "owner", "points": {}});
    conn.execute(
        "INSERT INTO joppa.state_entries(workspace, section, key, value) VALUES ($1, 'intake', 'in1', $2::text::jsonb)",
        &[&w.ws, &intake.to_string()],
    )
    .await?;
    conn.execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&w.ws])
        .await?;
    store::backfill_board(&w.pool).await?;
    drop(conn);
    w.start().await?;

    w.recorded(
        &"a".repeat(40),
        json!([{"test": "tests/x.rs > redaction_keeps_green", "status": "pass"}]),
    )
    .await?;
    ensure!(w.object("ac:a").await?["status"]["tone"] == "green");
    let r_before = w.object("requirement:r").await?["revision"]
        .as_i64()
        .context("r revision")?;

    // Warm search tables before redacting so both search_documents and search_revisions hold rows.
    let pre = ok(
        &w,
        "joppa_search",
        json!({"query": secret, "all_versions": true}),
    )
    .await?;
    ensure!(
        !pre["items"].as_array().context("pre")?.is_empty(),
        "{pre}"
    );

    let redact = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.alex)
        .json(&json!({"request_id": "redact-k7q", "command": {"op": "redact_object", "target": "ac:a", "fragment": secret, "reason": "Remove leaked token"}}))
        .send()
        .await?;
    let status = redact.status();
    let body = redact.text().await?;
    ensure!(status.is_success(), "redact status {status}: {body}");

    ensure!(w.object("ac:a").await?["status"]["tone"] == "green");
    ensure!(
        w.object("requirement:r").await?["revision"].as_i64() == Some(r_before),
        "requirement revision bumped on redaction"
    );

    let a_read = ok(
        &w,
        "joppa_objects",
        json!({"address": "ac:a", "fields": ["body", "comments"]}),
    )
    .await?;
    let a_text = a_read.to_string();
    ensure!(
        !a_text.contains(secret) && a_text.contains("[redacted]"),
        "{a_text}"
    );
    let history = ok(&w, "joppa_history", json!({"limit": 100})).await?;
    ensure!(!history.to_string().contains(secret), "{history}");
    let after = ok(
        &w,
        "joppa_search",
        json!({"query": secret, "all_versions": true}),
    )
    .await?;
    ensure!(
        after["items"].as_array().map(Vec::len) == Some(0),
        "{after}"
    );

    let conn = w.pool.get().await?;
    let leaked: i64 = conn
        .query_one(
            "SELECT
                (SELECT count(*) FROM joppa.search_documents d WHERE workspace=$1 AND to_jsonb(d)::text LIKE $2)
                + (SELECT count(*) FROM joppa.search_revisions r WHERE workspace=$1 AND to_jsonb(r)::text LIKE $2)
                + (SELECT count(*) FROM joppa.state_entries WHERE workspace=$1 AND section='intake' AND value::text LIKE $2)",
            &[&w.ws, &format!("%{secret}%")],
        )
        .await?
        .get(0);
    ensure!(leaked == 0, "leaked rows after redaction: {leaked}");
    Ok(())
}

// ac:m6goiyaq
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn alias_accepts_inner_slash_and_refuses_bad_slashes_or_41_chars() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let save = |id: &str, alias: &str| {
        json!({
            "alias": alias,
            "request_id": format!("alias-{id}"),
            "command": {
                "op": "save_object",
                "kind": "ac",
                "id": id,
                "revision": 0,
                "parent": "requirement:r",
                "data": {"title": id, "owner": "owner"},
                "reason": "Alias check"
            }
        })
    };
    ok(&w, "joppa_command", save("ok1", "crew/p1")).await?;
    let hist = ok(&w, "joppa_history", json!({"order": "newest", "limit": 1})).await?;
    ensure!(hist["items"][0]["alias"] == "crew/p1", "{hist}");

    let refusal = "alias has 1 to 40 letters, digits, \"-\", \"_\", \".\" or \"/\", with no \"/\" at the start, at the end or twice in a row";
    let too_long = "a".repeat(41);
    for (i, bad) in ["/p1", "p1/", "crew//p1", too_long.as_str()]
        .into_iter()
        .enumerate()
    {
        let res = w
            .mcp(&w.bot, "joppa_command", save(&format!("b{i}"), bad))
            .await?;
        ensure!(res == Err(refusal.into()), "{bad}: {res:?}");
    }
    Ok(())
}

// ac:bd6w8s1v
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn batch_save_catches_up_forced_revision_on_any_type_and_refuses_data_gap() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let tree = ok(&w, "joppa_tree", json!({})).await?;
    w.command(
        &w.owner,
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Custom types kr and proj",
            "put_types": {
                "kr": {"title": "KR", "role": "requirement", "status": "children", "root": true, "parents": []},
                "proj": {"title": "Proj", "role": "ac", "status": "children", "parents": ["kr"]}
            }
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    w.command(
        &w.bot,
        json!({"op": "save_object", "kind": "kr", "id": "k", "revision": 0, "parent": null, "data": {"title": "K1", "owner": "owner", "body": "Goal"}, "reason": "Seed parent"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;
    // Create proj:p1 at revision 1, bump to 2 and 3 so node N (proj:p1) stands at revision 3.
    for (rev, title) in [(0, "P1"), (1, "P2"), (2, "P3")] {
        w.command(
            &w.bot,
            json!({"op": "save_object", "kind": "proj", "id": "p1", "revision": rev, "parent": "kr:k", "data": {"title": title, "owner": "owner"}, "reason": "Step"}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
    }
    ensure!(w.object("proj:p1").await?["revision"] == 3);

    // Saving kr:k forces proj:p1 to revision 4. Saving proj:p1 with sent revision 3 in the same batch catches up to 5.
    let good = json!({
        "request_id": "forced-any-good",
        "operations": [
            {"command": {"op": "save_object", "kind": "kr", "id": "k", "revision": 1, "parent": null, "data": {"title": "K2", "owner": "owner", "body": "Goal"}, "reason": "Parent save"}},
            {"command": {"op": "save_object", "kind": "proj", "id": "p1", "revision": 3, "parent": "kr:k", "data": {"title": "P-final", "owner": "owner"}, "reason": "Child save"}}
        ]
    });
    ok(&w, "joppa_batch", good).await?;
    let p1 = w.object("proj:p1").await?;
    ensure!(
        p1["revision"] == 5 && p1["data"]["title"] == "P-final",
        "{p1}"
    );

    // A batch whose first op changes proj:p1 data and whose second op reuses revision 5 is refused as stale.
    let stale = json!({
        "request_id": "forced-any-stale",
        "operations": [
            {"command": {"op": "save_object", "kind": "proj", "id": "p1", "revision": 5, "parent": "kr:k", "data": {"title": "Changed", "owner": "owner"}, "reason": "Mutate"}},
            {"command": {"op": "save_object", "kind": "proj", "id": "p1", "revision": 5, "parent": "kr:k", "data": {"title": "Stale", "owner": "owner"}, "reason": "Reuse"}}
        ]
    });
    let err = w.mcp(&w.bot, "joppa_batch", stale).await?;
    ensure!(
        err.as_ref().is_err_and(|e| e.contains("stale")),
        "{err:?}"
    );
    ensure!(w.object("proj:p1").await?["data"]["title"] == "P-final");
    Ok(())
}
