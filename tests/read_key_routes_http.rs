//! ac:kqt767ea over a real server: a signed-in human member reads, creates and revokes the read
//! key of a workspace. Only the browser session of a member with the site origin passes. The
//! table keeps only the digest of the key, and the deletion of the workspace removes the row.
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use read_key::{Auth, call, create_key, is_key, key_path, key_row, purge, revoke_key, session};
use record_world::World;
use serde_json::{Value, json};

/// A new workspace on its own server: run `check` and purge the workspace.
async fn with_world(check: impl AsyncFn(&World) -> Result<()>) -> Result<()> {
    let w = record_world::world(&[]).await?;
    let result = check(&w).await;
    purge(&w.pool, &[&w.ws]).await?;
    result
}

/// One call of member alex to the read-key route with the site origin.
async fn alex(w: &World, cookie: &str, method: &str, tail: &str) -> Result<Value> {
    let auth = [Auth::Cookie(cookie), Auth::Origin(&w.root)];
    let (code, body) = call(
        &w.http,
        &w.root,
        method,
        &key_path(&w.ws, tail),
        &auth,
        None,
    )
    .await?;
    ensure!(code == 200, "{method} read-key{tail}: {code} {body}");
    Ok(body)
}

// ac:kqt767ea
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_reads_the_state_creates_a_key_and_revokes_it() -> Result<()> {
    with_world(async |w| {
        let m = session(w, "alex").await?;
        let first = alex(w, &m, "GET", "").await?;
        ensure!(
            first == json!({"active":false,"created_at":null}),
            "{first}"
        );
        let created = alex(w, &m, "POST", "").await?;
        let mut keys: Vec<&String> = created.as_object().context("object")?.keys().collect();
        keys.sort();
        ensure!(keys == ["created_at", "key"], "{created}");
        ensure!(created["key"].as_str().is_some_and(is_key), "{created}");
        ensure!(created["created_at"].is_string(), "{created}");
        let second = alex(w, &m, "GET", "").await?;
        let wanted = json!({"active":true,"created_at":created["created_at"]});
        ensure!(second == wanted, "{second}");
        let revoked = alex(w, &m, "POST", "/revoke").await?;
        ensure!(revoked == json!({"active":false}), "{revoked}");
        Ok(())
    })
    .await
}

// ac:kqt767ea
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_non_member_an_agent_a_bearer_header_or_a_foreign_origin_changes_no_row() -> Result<()> {
    with_world(async |w| {
        let m = session(w, "alex").await?;
        let n = session(w, "outsider").await?;
        let foreign = "http://evil.example";
        let refusals: [(&str, Vec<Auth>); 4] = [
            ("N", vec![Auth::Cookie(&n), Auth::Origin(&w.root)]),
            (
                "M with Authorization",
                vec![
                    Auth::Cookie(&m),
                    Auth::Bearer(&w.alex),
                    Auth::Origin(&w.root),
                ],
            ),
            (
                "M with a foreign Origin",
                vec![Auth::Cookie(&m), Auth::Origin(foreign)],
            ),
            (
                "A with its MCP token",
                vec![Auth::Bearer(&w.bot), Auth::Origin(&w.root)],
            ),
        ];
        // The refusals run once while W has a key and once while W has none.
        create_key(w, &m, &w.ws).await?;
        let with_key = key_row(w, &w.ws).await?;
        ensure!(with_key.0 == 1, "{with_key:?}");
        for row in [with_key, (0, None)] {
            if row.0 == 0 {
                revoke_key(w, &m, &w.ws).await?;
            }
            for (who, auth) in &refusals {
                let path = key_path(&w.ws, "");
                let (code, body) = call(&w.http, &w.root, "POST", &path, auth, None).await?;
                ensure!(matches!(code, 401 | 403), "{who}: {code} {body}");
                ensure!(body.get("key").is_none(), "{who}: {body}");
                let now = key_row(w, &w.ws).await?;
                ensure!(now == row, "{who}: the row changed to {now:?} from {row:?}");
            }
        }
        Ok(())
    })
    .await
}

// ac:kqt767ea
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_row_keeps_only_the_digest_and_no_event_or_log_line_holds_the_key() -> Result<()> {
    with_world(async |w| {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let server = read_key::logged(json!({ token: "bot" })).await?;
        let root = server.root.as_str();
        let m = session(w, "alex").await?;
        let auth = [Auth::Cookie(&m), Auth::Origin(root)];
        let (code, body) = call(&w.http, root, "POST", &key_path(&w.ws, ""), &auth, None).await?;
        ensure!(code == 200, "{code} {body}");
        let key = body["key"].as_str().context("key")?.to_owned();
        // The key also reaches the data route, a refused route and the state route.
        let bearer = [Auth::Bearer(&key)];
        call(&w.http, root, "POST", "/api/data", &bearer, Some(&json!({}))).await?;
        call(&w.http, root, "GET", "/api/workspaces", &bearer, None).await?;
        call(&w.http, root, "GET", &key_path(&w.ws, ""), &auth, None).await?;
        let conn = w.pool.get().await?;
        let sql = "SELECT token_sha256, to_jsonb(k)::text FROM joppa.workspace_read_keys k WHERE workspace=$1";
        let rows = conn.query(sql, &[&w.ws]).await?;
        ensure!(rows.len() == 1, "{} rows", rows.len());
        let (digest, row): (String, String) = (rows[0].get(0), rows[0].get(1));
        ensure!(digest == joppa::digest_hex::sha256_hex(&key), "token_sha256 {digest}");
        ensure!(!row.contains(&key), "the row holds the key: {row}");
        let sql = "SELECT count(*) FROM joppa.events e WHERE strpos(e::text, $1) > 0";
        let events: i64 = conn.query_one(sql, &[&key]).await?.get(0);
        ensure!(events == 0, "{events} events hold the key");
        let log = std::fs::read_to_string(&server.log)?;
        ensure!(!log.is_empty(), "the server log is empty");
        let lines: Vec<&str> = log.lines().filter(|l| l.contains(&key)).collect();
        ensure!(lines.is_empty(), "the server log holds the key: {lines:?}");
        Ok(())
    })
    .await
}

// ac:kqt767ea
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_deletion_of_the_workspace_removes_its_key_row() -> Result<()> {
    with_world(async |w| {
        let m = session(w, "alex").await?;
        create_key(w, &m, &w.ws).await?;
        ensure!(
            key_row(w, &w.ws).await?.0 == 1,
            "no key row before the deletion"
        );
        let owner = session(w, "owner").await?;
        let auth = [Auth::Cookie(&owner), Auth::Origin(&w.root)];
        let path = format!("/api/workspaces/{}/delete", w.ws);
        let (code, body) = call(&w.http, &w.root, "POST", &path, &auth, None).await?;
        ensure!(code == 200, "delete: {code} {body}");
        let row = key_row(w, &w.ws).await?;
        ensure!(row == (0, None), "after the deletion: {row:?}");
        Ok(())
    })
    .await
}
