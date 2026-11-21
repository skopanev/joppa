use super::*;

async fn mcp_alias(
    w: &World,
    root: &str,
    token: &str,
    alias: Option<&str>,
    tool: &str,
    args: Value,
) -> Result<(u16, Result<Value, String>)> {
    let mut req = w.http.post(format!("{root}/mcp")).bearer_auth(token).json(
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}}),
    );
    if let Some(alias) = alias {
        req = req.header("x-joppa-alias", alias);
    }
    let reply = req.send().await?;
    let code = reply.status().as_u16();
    let body: Value = reply.json().await.unwrap_or(Value::Null);
    if code != 200 {
        return Ok((code, Err(body.to_string())));
    }
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    let parsed = if body["result"]["isError"] == true {
        Err(text.to_owned())
    } else {
        Ok(serde_json::from_str(text)?)
    };
    Ok((200, parsed))
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_write_key_enforces_workspace_scope_schema_permissions_alias_and_editor_role()
-> Result<()> {
    let w = record_world::world(&["tests/x.rs > t1"]).await?;
    let child_ws = format!("child-{}", uuid::Uuid::new_v4().simple());
    let result = async {
        for (i, op) in [
            json!({"op":"workspace","title":"Child workspace"}),
            json!({"op":"member","id":"alex","role":"human"}),
        ]
        .into_iter()
        .enumerate()
        {
            let cmd: joppa::commands::Command = serde_json::from_value(op)?;
            joppa::store::execute(&w.pool, &child_ws, "owner", &format!("c-{i}"), &cmd).await?;
        }
        let cookie = session(&w, "alex").await?;
        let key = create_write_key(&w, &cookie, &w.ws).await?;
        let get_ok = w
            .http
            .get(format!("{}/mcp", w.root))
            .bearer_auth(&key)
            .send()
            .await?;
        ensure!(get_ok.status().as_u16() == 405, "{}", get_ok.status());
        // Enable key_write on test_results and ac.fields.stage, and attach child_ws on ac:a as owner.
        let tree = mcp_ok(&w, &w.owner, "joppa_tree", json!({})).await?;
        let mut tr = tree["schema"]["test_results"].clone();
        tr["key_write"] = json!(true);
        mcp_ok(
            &w,
            &w.owner,
            "joppa_command",
            json!({
                "request_id": "enable-key-write",
                "command": {
                    "op": "set_tree_schema",
                    "revision": tree["schema"]["revision"],
                    "reason": "Allow CI results and stage",
                    "test_results": tr,
                    "put_types": {
                        "ac": {
                            "title": "Criteria",
                            "role": "ac",
                            "status": "children",
                            "parents": ["requirement"],
                            "fields": {
                                "stage": {"type": "enum", "values": ["todo", "done"], "key_write": true}
                            }
                        }
                    }
                }
            }),
        )
        .await?;
        mcp_ok(
            &w,
            &w.owner,
            "joppa_command",
            json!({
                "request_id": "set-child-ws",
                "command": {"op":"save_object","kind":"ac","id":"a","revision":1,"data":{"workspace":&child_ws},"reason":"Link child workspace"}
            }),
        )
        .await?;
        let (_, rec) = mcp_alias(
            &w,
            &w.root,
            &key,
            Some("ci/cloud-build"),
            "joppa_record_results",
            json!({"workspace": &w.ws, "commit": "b".repeat(40), "results": [{"test": "tests/x.rs > t1", "status": "pass"}]}),
        )
        .await?;
        let rec = rec.map_err(anyhow::Error::msg)?;
        ensure!(rec["unbound"] == json!([]), "{rec}");
        let green = mcp_ok(&w, &key, "joppa_objects", json!({"address":"ac:a"})).await?;
        ensure!(green["status"]["tone"] == "green", "{green}");
        let rev = green["revision"].as_u64().context("rev")?;
        // Updating stage preserves stored data.workspace, keeps ac:a green, and replays idempotently.
        let save_args = json!({
            "workspace": &w.ws,
            "request_id": "set-stage-done",
            "command": {"op": "save_object", "kind": "ac", "id": "a", "revision": rev, "data": {"stage": "done"}, "reason": "All tests passed"}
        });
        let (_, saved) = mcp_alias(&w, &w.root, &key, Some("ci/cloud-build"), "joppa_command", save_args.clone()).await?;
        saved.map_err(anyhow::Error::msg)?;
        let (_, retry) = mcp_alias(&w, &w.root, &key, Some("ci/cloud-build"), "joppa_command", save_args).await?;
        ensure!(retry.map_err(anyhow::Error::msg)?["replayed"] == true, "expected replayed true");
        let after = mcp_ok(&w, &key, "joppa_objects", json!({"address":"ac:a"})).await?;
        ensure!(
            after["data"]["stage"] == "done"
                && after["data"]["workspace"] == child_ws
                && after["status"]["tone"] == "green",
            "{after}"
        );
        let hist = mcp_ok(&w, &key, "joppa_objects", json!({"address":"ac:a","history":true})).await?;
        let last = hist["items"].as_array().and_then(|i| i.last()).context("last")?;
        ensure!(last["actor"] == "alex" && last["alias"] == "ci/cloud-build", "{last}");
        Ok(())
    }
    .await;
    purge(&w.pool, &[&w.ws, &child_ws]).await?;
    result
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_write_key_refuses_other_workspaces_item_id_leaks_unlisted_fields_replays_and_revoked_access()
-> Result<()> {
    let w = record_world::world(&["tests/x.rs > t1"]).await?;
    let other_ws = format!("other-{}", uuid::Uuid::new_v4().simple());
    let alias_slug = format!("slug-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let result = async {
        for (i, op) in [
            json!({"op":"workspace","title":"Other workspace"}),
            json!({"op":"update_workspace","workspace_id":&alias_slug,"reason":"Set slug"}),
            json!({"op":"member","id":"alex","role":"human"}),
        ]
        .into_iter()
        .enumerate()
        {
            let cmd: joppa::commands::Command = serde_json::from_value(op)?;
            joppa::store::execute(&w.pool, &other_ws, "owner", &format!("o-{i}"), &cmd).await?;
        }
        joppa::store::execute_remote_input(
            &w.pool,
            &other_ws,
            "owner",
            "o-seed-note",
            &json!({"op":"save_object","kind":"note","id":"n1","revision":0,"parent":null,"data":{"title":"Other note","owner":"owner","body":"Private"},"reason":"Seed"}),
        )
        .await?;
        let conn = w.pool.get().await?;
        let w1_item: String = conn.query_one("SELECT id FROM joppa.item_ids WHERE workspace=$1 AND kind='ac' AND local_id='a' LIMIT 1", &[&w.ws]).await?.get(0);
        let w2_item: String = conn.query_one("SELECT id FROM joppa.item_ids WHERE workspace=$1 AND kind='note' AND local_id='n1' LIMIT 1", &[&other_ws]).await?.get(0);
        let cookie = session(&w, "alex").await?;
        let key = create_write_key(&w, &cookie, &w.ws).await?;
        // Cross-workspace public item ID alongside a valid W1 item ID gives missing, identical to a random ID.
        let (_, pair_w2) = mcp_alias(&w, &w.root, &key, None, "joppa_objects", json!({"address": format!("{w1_item},{w2_item}")})).await?;
        let (_, pair_fake) = mcp_alias(&w, &w.root, &key, None, "joppa_objects", json!({"address": format!("{w1_item},nope-N-ZZZZZZZZ")})).await?;
        ensure!(pair_w2.map_err(anyhow::Error::msg)?["missing"] == json!([w2_item]), "expected w2_item in missing");
        ensure!(pair_fake.map_err(anyhow::Error::msg)?["missing"] == json!(["nope-N-ZZZZZZZZ"]), "expected fake in missing");
        // Non-/mcp routes and disallowed tools fail.
        for (method, path, body) in [
            ("GET", "/api/workspaces".to_owned(), None),
            ("POST", "/api/data".to_owned(), Some(json!({}))),
            ("POST", format!("/api/workspaces/{}/commands", w.ws), Some(json!({"request_id":"x","command":{"op":"note","target":"ac:a","id":"c1","text":"Hi"}}))),
        ] {
            let (code, _) = call(&w.http, &w.root, method, &path, &[Auth::Bearer(&key)], body.as_ref()).await?;
            ensure!(matches!(code, 401 | 403), "{method} {path}: {code}");
        }
        for (tool, args) in [
            ("joppa_objects", json!({"workspace": &other_ws})),
            ("joppa_list", json!({})),
            ("joppa_search", json!({"workspace": &w.ws, "query": "R", "all_workspaces": true})),
            ("joppa_attach", json!({"workspace": &w.ws, "target": "ac:a", "filename": "a.txt", "content": "x"})),
            ("joppa_history_mark", json!({"workspace": &w.ws, "position": 1, "reason": "r"})),
            ("joppa_record_results", json!({"workspace": &w.ws, "commit": "a".repeat(40), "results": [{"test": "tests/x.rs > t1", "status": "pass"}]})),
        ] {
            let (_, res) = mcp_alias(&w, &w.root, &key, None, tool, args).await?;
            ensure!(res.is_err(), "{tool} succeeded: {res:?}");
        }
        // Alex saves a title change over OAuth/token, then a write key cannot replay that receipt or leak alias_slug UUID.
        mcp_ok(&w, &w.alex, "joppa_command", json!({"request_id":"alex-title-edit","command":{"op":"save_object","kind":"ac","id":"a","revision":1,"data":{"title":"A2"},"reason":"Edit title"}})).await?;
        let (_, replay_bad) = mcp_alias(&w, &w.root, &key, None, "joppa_command", json!({"workspace":&w.ws,"request_id":"alex-title-edit","command":{"op":"save_object","kind":"ac","id":"a","revision":1,"data":{"title":"A2"},"reason":"Edit title"}})).await?;
        ensure!(replay_bad.is_err(), "replayed non-key_write receipt: {replay_bad:?}");
        let (_, ws_leak) = mcp_alias(&w, &w.root, &key, None, "joppa_command", json!({"workspace":&w.ws,"request_id":"try-ws","command":{"op":"save_object","kind":"ac","id":"a","revision":2,"data":{"workspace":&alias_slug},"reason":"Probe"}})).await?;
        let leak_err = ws_leak.err().context("expected error")?;
        ensure!(!leak_err.contains(&other_ws), "leaked UUID in error: {leak_err}");
        for (label, cmd) in [
            ("clean save", json!({"op":"save_object","kind":"ac","id":"a","revision":2,"clean":true,"data":{"title":"A2"},"reason":"r"})),
            ("parent change", json!({"op":"save_object","kind":"ac","id":"a","revision":2,"parent":null,"data":{"title":"A2"},"reason":"r"})),
            ("relations change", json!({"op":"save_object","kind":"ac","id":"a","revision":2,"relations":{},"data":{"title":"A2"},"reason":"r"})),
            ("create object", json!({"op":"save_object","kind":"ac","id":"b","revision":0,"parent":"requirement:r","data":{"title":"B"},"reason":"r"})),
            ("archive op", json!({"op":"archive_object","target":"ac:a","revision":2,"reason":"r"})),
        ] {
            let (_, res) = mcp_alias(&w, &w.root, &key, None, "joppa_command", json!({"workspace":&w.ws,"request_id":format!("d-{label}"),"command":cmd})).await?;
            ensure!(res.is_err(), "{label} succeeded");
        }
        // Demoting alex to viewer gives 401, restoring alex and revoking the key gives 401.
        joppa::store::set_role(&w.pool, &w.ws, "owner", Some("alex"), None, "viewer").await?;
        let (demoted, _) = mcp_alias(&w, &w.root, &key, None, "joppa_tree", json!({"workspace": &w.ws})).await?;
        ensure!(demoted == 401, "demoted gave {demoted}");
        joppa::store::set_role(&w.pool, &w.ws, "owner", Some("alex"), None, "human").await?;
        revoke_write_key(&w, &cookie, &w.ws).await?;
        let (revoked, _) = mcp_alias(&w, &w.root, &key, None, "joppa_tree", json!({"workspace": &w.ws})).await?;
        ensure!(revoked == 401, "revoked gave {revoked}");
        // JOPPA_ACCESS_RULES_FILE excluding a Google-enrolled Editor returns 401 on /mcp ping.
        let g_actor = format!("ed-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
        let g_email = format!("{g_actor}@example.org");
        joppa::store::enroll(
            &w.pool,
            &g_actor,
            &g_actor,
            &g_email,
            None,
            &std::collections::BTreeSet::from([w.ws.clone()]),
        )
        .await?;
        let rules_path =
            std::env::temp_dir().join(format!("rules-{}.json", uuid::Uuid::new_v4().simple()));
        std::fs::write(&rules_path, json!({"emails":{&g_email:[&w.ws]}}).to_string())?;
        let (_spare, spare_root) = w
            .spare(&[("JOPPA_ACCESS_RULES_FILE", rules_path.to_string_lossy().into_owned())])
            .await?;
        let g_cookie = session(&w, &g_actor).await?;
        let auth = [Auth::Cookie(&g_cookie), Auth::Origin(&spare_root)];
        let (code, body) = call(&w.http, &spare_root, "POST", &write_key_path(&w.ws, ""), &auth, None).await?;
        ensure!(code == 200, "spare write-key create: {code} {body}");
        let g_key = body["key"].as_str().context("g_key")?.to_owned();
        std::fs::write(&rules_path, json!({"emails":{&g_email:[]}}).to_string())?;
        let ping_code = w
            .http
            .post(format!("{spare_root}/mcp"))
            .bearer_auth(&g_key)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"ping"}))
            .send()
            .await?
            .status()
            .as_u16();
        let _ = std::fs::remove_file(&rules_path);
        conn.execute(
            "DELETE FROM joppa.google_identities WHERE actor=$1",
            &[&g_actor],
        )
        .await?;
        ensure!(ping_code == 401, "access-rules excluded creator gave {ping_code}");
        Ok(())
    }
    .await;
    purge(&w.pool, &[&w.ws, &other_ws]).await?;
    result
}
