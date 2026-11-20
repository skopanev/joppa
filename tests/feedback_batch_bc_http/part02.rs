// Part 2 of tests/feedback_batch_bc_http.rs: ac:y3jrsfze, ac:qiq0u7w9, ac:478njhfy.

// ac:y3jrsfze
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn archived_search_hit_gives_present_false_and_ranks_below_live_hit() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let tree = ok(&w, "joppa_tree", json!({})).await?;
    w.command(
        &w.owner,
        json!({
            "op": "set_tree_schema",
            "revision": tree["schema"]["revision"],
            "reason": "Add domain type",
            "put_types": {
                "domain": {"title": "Domain", "role": "group", "status": "children", "root": true, "parents": []}
            }
        }),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    // Name archived domain "a_arch" and live domain "z_live" so alphabetical order would put archived first without live-first sort.
    ok(
        &w,
        "joppa_batch",
        json!({
            "request_id": "seed-search-live-arch",
            "operations": [
                {"command": {"op": "save_object", "kind": "domain", "id": "a_arch", "revision": 0, "parent": null, "data": {"title": "Invoice reminder", "owner": "owner", "body": "Send reminder after 7 days"}, "reason": "Create archived candidate"}},
                {"command": {"op": "save_object", "kind": "domain", "id": "z_live", "revision": 0, "parent": null, "data": {"title": "Invoice reminder", "owner": "owner", "body": "Send reminder after 7 days"}, "reason": "Create live candidate"}},
                {"command": {"op": "archive_object", "target": "domain:a_arch", "revision": 1, "reason": "Superseded"}}
            ]
        }),
    )
    .await?;

    let found = ok(
        &w,
        "joppa_search",
        json!({"query": "Invoice reminder", "all_versions": true}),
    )
    .await?;
    let items = found["items"].as_array().context("search items")?;
    ensure!(
        items.len() >= 2
            && items[0]["address"] == "domain:z_live"
            && items[0].get("present").is_none()
            && items[1]["address"] == "domain:a_arch"
            && items[1]["present"] == false,
        "{found}"
    );
    Ok(())
}

// ac:qiq0u7w9
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn comments_active_and_comments_latest_skip_withdrawn_and_keep_comments_intact() -> Result<()>
{
    let w = record_world::world(&[]).await?;
    for id in ["d", "e"] {
        w.command(
            &w.bot,
            json!({"op": "save_object", "kind": "requirement", "id": id, "revision": 0, "parent": null, "data": {"title": id.to_uppercase(), "owner": "owner", "body": "Rule"}, "reason": "Seed"}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
    }

    let mut ops: Vec<Value> = (0..30)
        .map(|i| {
            json!({"command": {"op": "note", "target": "requirement:d", "id": format!("c{i:02}"), "text": format!("Note {i}")}})
        })
        .collect();
    ops.push(json!({"command": {"op": "note", "target": "requirement:d", "id": "c30", "text": "Withdrawn on d"}}));
    ops.push(json!({"command": {"op": "withdraw_note", "target": "requirement:d", "id": "c30", "reason": "Noise"}}));
    ops.push(json!({"command": {"op": "note", "target": "requirement:e", "id": "c00", "text": "Withdrawn on e"}}));
    ops.push(json!({"command": {"op": "withdraw_note", "target": "requirement:e", "id": "c00", "reason": "Noise"}}));
    ok(
        &w,
        "joppa_batch",
        json!({"request_id": "seed-comments-active-latest", "operations": ops}),
    )
    .await?;

    let active_p1 = ok(
        &w,
        "joppa_objects",
        json!({"address": "requirement:d", "fields": ["comments_active"]}),
    )
    .await?;
    ensure!(
        active_p1["comments_active"]["items"]
            .as_array()
            .map(Vec::len)
            == Some(25)
            && active_p1["comments_active"]["total"] == 30
            && active_p1["comments_active"]["next_cursor"] == "25",
        "{active_p1}"
    );

    let active_p2 = ok(
        &w,
        "joppa_objects",
        json!({"address": "requirement:d", "fields": ["comments_active"], "comments_cursor": "25"}),
    )
    .await?;
    let p2_items = active_p2["comments_active"]["items"]
        .as_array()
        .context("p2 items")?;
    ensure!(
        p2_items.len() == 5
            && p2_items[0]["id"] == "c25"
            && p2_items[4]["id"] == "c29"
            && active_p2["comments_active"]["total"] == 30,
        "{active_p2}"
    );

    let latest_d = ok(
        &w,
        "joppa_objects",
        json!({"address": "requirement:d", "fields": ["comments_latest"]}),
    )
    .await?;
    let latest_ids: Vec<&str> = latest_d["comments_latest"]
        .as_array()
        .context("latest array")?
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    ensure!(
        latest_ids == vec!["c25", "c26", "c27", "c28", "c29"],
        "{latest_d}"
    );

    let empty_e = ok(
        &w,
        "joppa_objects",
        json!({"address": "requirement:e", "fields": ["comments_active", "comments_latest"]}),
    )
    .await?;
    ensure!(
        empty_e == json!({"comments_active": null, "comments_latest": null}),
        "{empty_e}"
    );

    let listed = ok(
        &w,
        "joppa_objects",
        json!({"kind": "requirement", "fields": ["comments_latest"]}),
    )
    .await?;
    let items = listed["items"].as_array().context("list items")?;
    let d_item = items
        .iter()
        .find(|i| i["address"] == "requirement:d")
        .context("d in list")?;
    let e_item = items
        .iter()
        .find(|i| i["address"] == "requirement:e")
        .context("e in list")?;
    ensure!(
        d_item["comments_latest"] == latest_d["comments_latest"]
            && e_item["comments_latest"].is_null(),
        "{listed}"
    );

    let raw_all = ok(
        &w,
        "joppa_objects",
        json!({"address": "requirement:d", "fields": ["comments"]}),
    )
    .await?;
    ensure!(raw_all["comments"]["total"] == 31, "{raw_all}");
    Ok(())
}

// ac:478njhfy
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn mcp_reads_separate_authored_data_and_computed_while_http_merges() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > computed_sep"]).await?;

    // First give ac an own stage field so revision 1 of ac:a authors stage="to_prepare", then switch stage on ac to from="status".
    let tree = ok(&w, "joppa_tree", json!({})).await?;
    let mut ac_own = tree["schema"]["types"]["ac"].clone();
    ac_own["fields"] = json!({"stage": {"type": "enum", "values": ["to_prepare", "to_implement", "done"]}});
    w.command(
        &w.owner,
        json!({"op": "set_tree_schema", "revision": tree["schema"]["revision"], "put_types": {"ac": ac_own}, "reason": "Own stage first"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    w.command(
        &w.bot,
        json!({"op": "save_object", "kind": "ac", "id": "a", "revision": 1, "parent": "requirement:r", "data": {"title": "A", "owner": "owner", "stage": "to_prepare"}, "reason": "Author stage"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    let tree = ok(&w, "joppa_tree", json!({})).await?;
    let mut ac_comp = tree["schema"]["types"]["ac"].clone();
    ac_comp["fields"] = json!({
        "stage": {
            "type": "enum",
            "values": ["to_prepare", "to_implement", "done"],
            "from": "status",
            "map": {"orange": "to_prepare", "blue": "to_implement", "green": "done"}
        }
    });
    w.command(
        &w.owner,
        json!({"op": "set_tree_schema", "revision": tree["schema"]["revision"], "put_types": {"ac": ac_comp}, "reason": "Computed stage"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    // Also create ac:b under computed stage so ac:b has zero authored stage in data.
    w.command(
        &w.bot,
        json!({"op": "save_object", "kind": "ac", "id": "b", "revision": 0, "parent": "requirement:r", "data": {"title": "B", "owner": "owner"}, "reason": "Pure computed ac"}),
    )
    .await?
    .map_err(anyhow::Error::msg)?;

    w.recorded(
        &"c".repeat(40),
        json!([{"test": "tests/x.rs > computed_sep", "status": "pass"}]),
    )
    .await?;

    let card_a = ok(&w, "joppa_objects", json!({"address": "ac:a"})).await?;
    ensure!(
        card_a["data"]["stage"] == "to_prepare" && card_a["computed"] == json!({"stage": "done"}),
        "{card_a}"
    );
    let card_b = ok(&w, "joppa_objects", json!({"address": "ac:b"})).await?;
    ensure!(
        card_b["data"].get("stage").is_none() && card_b["computed"] == json!({"stage": "to_prepare"}),
        "{card_b}"
    );

    let f_comp = ok(
        &w,
        "joppa_objects",
        json!({"address": "ac:a", "fields": ["computed"]}),
    )
    .await?;
    let f_dot = ok(
        &w,
        "joppa_objects",
        json!({"address": "ac:a", "fields": ["computed.stage"]}),
    )
    .await?;
    let f_bare_a = ok(
        &w,
        "joppa_objects",
        json!({"address": "ac:a", "fields": ["stage"]}),
    )
    .await?;
    let f_bare_b = ok(
        &w,
        "joppa_objects",
        json!({"address": "ac:b", "fields": ["stage"]}),
    )
    .await?;
    let f_data = ok(
        &w,
        "joppa_objects",
        json!({"address": "ac:a", "fields": ["data"]}),
    )
    .await?;
    ensure!(
        f_comp == json!({"computed": {"stage": "done"}})
            && f_dot == json!({"computed.stage": "done"})
            && f_bare_a == json!({"stage": "to_prepare"})
            && f_bare_b == json!({"stage": "to_prepare"})
            && f_data["data"]["stage"] == "to_prepare",
        "{f_comp} / {f_dot} / {f_bare_a} / {f_bare_b} / {f_data}"
    );

    let rev1 = ok(
        &w,
        "joppa_objects",
        json!({"address": "ac:a", "revision": 1, "fields": ["computed"]}),
    )
    .await?;
    ensure!(rev1 == json!({"computed": {}}), "{rev1}");

    let list = ok(
        &w,
        "joppa_objects",
        json!({"kind": "ac", "fields": ["computed"]}),
    )
    .await?;
    ensure!(
        list["items"]
            == json!([
                {"address": "ac:a", "computed": {"stage": "done"}},
                {"address": "ac:b", "computed": {"stage": "to_prepare"}}
            ]),
        "{list}"
    );

    let http: Value = w
        .http
        .get(format!("{}/api/workspaces/{}/objects?address=ac:a", w.root, w.ws))
        .bearer_auth(&w.bot)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(http["document"]["data"]["stage"] == "done", "{http}");
    Ok(())
}
