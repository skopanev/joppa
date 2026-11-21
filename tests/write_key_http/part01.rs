use super::*;

async fn with_world(check: impl AsyncFn(&World) -> Result<()>) -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t1"]).await?;
    let result = check(&w).await;
    purge(&w.pool, &[&w.ws]).await?;
    result
}

async fn alex_key(w: &World, cookie: &str, method: &str, tail: &str) -> Result<Value> {
    let auth = [Auth::Cookie(cookie), Auth::Origin(&w.root)];
    let path = write_key_path(&w.ws, tail);
    let (code, body) = call(&w.http, &w.root, method, &path, &auth, None).await?;
    ensure!(code == 200, "{method} write-key{tail}: {code} {body}");
    Ok(body)
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_reads_creates_replaces_and_revokes_the_write_key() -> Result<()> {
    with_world(async |w| {
        let m = session(w, "alex").await?;
        let first = alex_key(w, &m, "GET", "").await?;
        ensure!(
            first == json!({"active":false,"created_at":null}),
            "{first}"
        );
        let created = alex_key(w, &m, "POST", "").await?;
        let key1 = created["key"].as_str().context("key")?.to_owned();
        ensure!(joppa::write_key::is_write_key(&key1), "{created}");
        let second = alex_key(w, &m, "GET", "").await?;
        ensure!(
            second == json!({"active":true,"created_at":created["created_at"]}),
            "{second}"
        );
        let replaced = alex_key(w, &m, "POST", "").await?;
        let key2 = replaced["key"].as_str().context("key")?.to_owned();
        ensure!(
            key1 != key2 && joppa::write_key::is_write_key(&key2),
            "{replaced}"
        );
        let (old_code, _) = w
            .mcp_raw(Some(&key1), "joppa_tree", json!({"workspace": w.ws}))
            .await?;
        ensure!(old_code == 401, "replaced key gave {old_code}");
        // Only the SHA-256 digest is stored, and neither events nor logs contain the secret.
        let token = uuid::Uuid::new_v4().simple().to_string();
        let server = read_key::logged(json!({ token: "bot" })).await?;
        let auth = [Auth::Cookie(&m), Auth::Origin(&server.root)];
        let (code, body) = call(
            &w.http,
            &server.root,
            "POST",
            &write_key_path(&w.ws, ""),
            &auth,
            None,
        )
        .await?;
        ensure!(code == 200, "{code} {body}");
        let key = body["key"].as_str().context("key")?.to_owned();
        let bearer = [Auth::Bearer(&key)];
        call(&w.http, &server.root, "GET", "/mcp", &bearer, None).await?;
        call(&w.http, &server.root, "GET", "/api/workspaces", &bearer, None).await?;
        let conn = w.pool.get().await?;
        let row = conn
            .query_one(
                "SELECT token_sha256, created_by, to_jsonb(k)::text FROM joppa.workspace_write_keys k WHERE workspace=$1",
                &[&w.ws],
            )
            .await?;
        let (digest, creator, text): (String, String, String) =
            (row.get(0), row.get(1), row.get(2));
        ensure!(
            digest == joppa::digest_hex::sha256_hex(&key) && creator == "alex",
            "{digest} {creator}"
        );
        ensure!(!text.contains(&key), "row holds secret: {text}");
        let events: i64 = conn
            .query_one(
                "SELECT count(*) FROM joppa.events e WHERE strpos(e::text, $1) > 0",
                &[&key],
            )
            .await?
            .get(0);
        ensure!(events == 0, "{events} events hold the key");
        let log = std::fs::read_to_string(&server.log)?;
        ensure!(!log.is_empty() && !log.contains(&key), "log holds secret");
        let revoked = alex_key(w, &m, "POST", "/revoke").await?;
        ensure!(revoked == json!({"active":false}), "{revoked}");
        ensure!(
            write_key_row(w, &w.ws).await? == (0, None),
            "row remained after revoke"
        );
        let _ = create_write_key(w, &m, &w.ws).await?;
        ensure!(
            write_key_row(w, &w.ws).await?.0 == 1,
            "missing row before delete"
        );
        read_key::command(
            w,
            &w.owner,
            json!({"op":"delete_workspace","reason":"Clean up"}),
        )
        .await?;
        ensure!(
            write_key_row(w, &w.ws).await? == (0, None),
            "row remained after delete"
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn write_key_routes_refuse_viewers_agents_bearer_headers_and_foreign_origins() -> Result<()> {
    with_world(async |w| {
        let m = session(w, "alex").await?;
        let n = session(w, "outsider").await?;
        let _ = create_write_key(w, &m, &w.ws).await?;
        let before = write_key_row(w, &w.ws).await?;
        joppa::store::set_role(
            &w.pool,
            &w.ws,
            "owner",
            Some("another-member"),
            None,
            "viewer",
        )
        .await?;
        let v_cookie = session(w, "another-member").await?;
        for auth in [
            vec![Auth::Cookie(&n), Auth::Origin(&w.root)],
            vec![Auth::Cookie(&v_cookie), Auth::Origin(&w.root)],
            vec![
                Auth::Cookie(&m),
                Auth::Bearer(&w.alex),
                Auth::Origin(&w.root),
            ],
            vec![Auth::Cookie(&m), Auth::Origin("http://evil.example")],
            vec![Auth::Bearer(&w.bot), Auth::Origin(&w.root)],
        ] {
            let path = write_key_path(&w.ws, "");
            let (code, body) = call(&w.http, &w.root, "POST", &path, &auth, None).await?;
            ensure!(
                matches!(code, 401 | 403) && body.get("key").is_none(),
                "{code} {body}"
            );
            ensure!(write_key_row(w, &w.ws).await? == before, "row changed");
        }
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn set_tree_schema_accepts_key_write_on_test_results_and_own_fields_and_refuses_computed_fields()
-> Result<()> {
    with_world(async |w| {
        let tree = mcp_ok(w, &w.owner, "joppa_tree", json!({})).await?;
        let rev = tree["schema"]["revision"].as_u64().context("rev")?;
        let mut results = tree["schema"]["test_results"].clone();
        results["key_write"] = json!(true);
        let bad = w
            .mcp(
                &w.owner,
                "joppa_command",
                json!({
                    "request_id": "bad-computed-key-write",
                    "command": {
                        "op": "set_tree_schema",
                        "revision": rev,
                        "reason": "Try computed key_write",
                        "put_types": {
                            "ac": {
                                "title": "Criteria",
                                "role": "ac",
                                "status": "children",
                                "parents": ["requirement"],
                                "fields": {
                                    "stage": {
                                        "type": "enum",
                                        "values": ["todo", "done"],
                                        "from": "status",
                                        "map": {"green": "done", "blue": "todo"},
                                        "key_write": true
                                    }
                                }
                            }
                        }
                    }
                }),
            )
            .await?;
        ensure!(
            bad.as_ref()
                .is_err_and(|e| e.contains("stage: only an own field takes key_write")),
            "{bad:?}"
        );
        mcp_ok(
            w,
            &w.owner,
            "joppa_command",
            json!({
                "request_id": "good-key-write",
                "command": {
                    "op": "set_tree_schema",
                    "revision": rev,
                    "reason": "Enable key_write on test_results and stage",
                    "test_results": results,
                    "put_types": {
                        "ac": {
                            "title": "Criteria",
                            "role": "ac",
                            "status": "children",
                            "parents": ["requirement"],
                            "fields": {
                                "stage": {"type": "enum", "values": ["todo", "done"], "key_write": true},
                                "note": {"type": "text", "key_write": false}
                            }
                        }
                    }
                }
            }),
        )
        .await?;
        let updated = mcp_ok(w, &w.owner, "joppa_tree", json!({})).await?;
        ensure!(
            updated["schema"]["test_results"]["key_write"] == true,
            "{updated}"
        );
        ensure!(
            updated["schema"]["types"]["ac"]["fields"]["stage"]["key_write"] == true,
            "{updated}"
        );
        ensure!(
            updated["schema"]["types"]["ac"]["fields"]["note"]
                .get("key_write")
                .is_none(),
            "{updated}"
        );
        Ok(())
    })
    .await
}
