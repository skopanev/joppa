//! ac:8v86osn3 over a real server: a read key reads only POST /api/data of its own workspace.
//! Every other route refuses it, a bad credential gets 401, and no request writes.
#[path = "common/read_key.rs"]
mod read_key;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use read_key::{Auth, call, create_key, data, purge, revoke_key, session};
use record_world::World;
use serde_json::{Value, json};

/// W1 is the workspace of the world under a new ID; its first ID stays its alias. W2 is a
/// second workspace. K1 is the read key of W1, and R is a revoked key of W1.
struct Scope<'a> {
    w: &'a World,
    w1: String,
    w2: String,
    k1: String,
    r: String,
}

/// Build the scope, run `check` and purge both workspaces.
async fn with_scope(check: impl AsyncFn(&Scope<'_>) -> Result<()>) -> Result<()> {
    let w = record_world::world(&[]).await?;
    let w1 = format!("scope-{}", uuid::Uuid::new_v4().simple());
    let w2 = format!("other-{}", uuid::Uuid::new_v4().simple());
    let result = async {
        let rename = json!({"op":"update_workspace","title":"Read key scope","workspace_id":w1,
            "reason":"Give the workspace a new ID"});
        read_key::command(&w, &w.owner, rename).await?;
        let other = json!({"op":"workspace","title":"Other workspace"});
        let id = uuid::Uuid::new_v4().to_string();
        joppa::store::execute_input(&w.pool, &w2, "owner", &id, &other).await?;
        let cookie = session(&w, "alex").await?;
        let r = create_key(&w, &cookie, &w1).await?;
        revoke_key(&w, &cookie, &w1).await?;
        let k1 = create_key(&w, &cookie, &w1).await?;
        let (w1, w2) = (w1.clone(), w2.clone());
        check(&Scope {
            w: &w,
            w1,
            w2,
            k1,
            r,
        })
        .await
    }
    .await;
    purge(&w.pool, &[&w1, &w.ws, &w2]).await?;
    result
}

/// The three requests of K1 that read W1: no workspace, the ID of W1 and the alias of W1.
fn own_bodies(s: &Scope<'_>) -> [Value; 3] {
    [
        json!({}),
        json!({"workspace": s.w1}),
        json!({"workspace": s.w.ws}),
    ]
}

/// K1 on each route other than POST /api/data: the route name, the code and the body.
async fn other_routes(s: &Scope<'_>) -> Result<Vec<(String, u16, Value)>> {
    let (w, k1) = (s.w, s.k1.as_str());
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}});
    let mut out = vec![];
    for (method, path, body) in [
        ("POST", "/mcp".to_owned(), Some(&init)),
        ("GET", "/api/workspaces".to_owned(), None),
        ("GET", format!("/api/workspaces/{}/objects", s.w1), None),
        ("GET", read_key::key_path(&s.w1, ""), None),
    ] {
        let (code, reply) =
            call(&w.http, &w.root, method, &path, &[Auth::Bearer(k1)], body).await?;
        out.push((format!("{method} {path}"), code, reply));
    }
    Ok(out)
}

/// Each bad credential on POST /api/data: the name, the code and the body.
async fn bad_credentials(s: &Scope<'_>) -> Result<Vec<(String, u16, Value)>> {
    let unknown = format!("jpr_{}", "0123456789abcdef".repeat(4));
    let mut out = vec![];
    for (name, key) in [
        ("none", None),
        ("Bearer abc", Some("abc")),
        ("an unknown key", Some(unknown.as_str())),
        ("the revoked key R", Some(s.r.as_str())),
        ("the MCP token T", Some(s.w.bot.as_str())),
    ] {
        let (code, reply) = data(s.w, key, &json!({})).await?;
        out.push((name.to_owned(), code, reply));
    }
    Ok(out)
}

/// The count of joppa.events rows and the revision of each object of both workspaces.
async fn snapshot(s: &Scope<'_>) -> Result<(i64, Vec<(String, String, i32)>)> {
    let ids = vec![s.w1.clone(), s.w.ws.clone(), s.w2.clone()];
    let conn = s.w.pool.get().await?;
    let sql = "SELECT count(*) FROM joppa.events WHERE workspace = ANY($1)";
    let events: i64 = conn.query_one(sql, &[&ids]).await?.get(0);
    let sql = "SELECT workspace, address, revision FROM joppa.objects WHERE workspace = ANY($1) ORDER BY 1, 2";
    let rows = conn.query(sql, &[&ids]).await?;
    Ok((
        events,
        rows.iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect(),
    ))
}

// ac:8v86osn3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_read_key_reads_its_workspace_with_no_name_its_id_or_its_alias() -> Result<()> {
    with_scope(async |s| {
        for body in own_bodies(s) {
            let (code, reply) = data(s.w, Some(&s.k1), &body).await?;
            ensure!(code == 200, "{body}: {code} {reply}");
            ensure!(reply["workspace"] == json!(s.w1), "{body}: {reply}");
            let mut found = read_key::addresses(&reply);
            found.sort();
            ensure!(found == ["ac:a", "requirement:r"], "{body}: {found:?}");
        }
        Ok(())
    })
    .await
}

// ac:8v86osn3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_read_key_that_names_another_workspace_gets_403() -> Result<()> {
    with_scope(async |s| {
        let body = json!({"workspace": s.w2});
        let (code, reply) = data(s.w, Some(&s.k1), &body).await?;
        ensure!(
            code == 403 && reply.get("items").is_none(),
            "{code} {reply}"
        );
        Ok(())
    })
    .await
}

// ac:8v86osn3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_read_key_gets_401_and_no_data_on_mcp_and_every_other_api_route() -> Result<()> {
    with_scope(async |s| {
        for (route, code, reply) in other_routes(s).await? {
            let text = reply.to_string();
            let leak = ["requirement:r", "ac:a", "Read key scope", "\"result\""];
            ensure!(code == 401, "{route}: {code} {reply}");
            ensure!(!leak.iter().any(|l| text.contains(l)), "{route}: {reply}");
        }
        Ok(())
    })
    .await
}

// ac:8v86osn3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_missing_foreign_unknown_revoked_or_mcp_credential_gets_401_on_the_data_route()
-> Result<()> {
    with_scope(async |s| {
        for (name, code, reply) in bad_credentials(s).await? {
            ensure!(
                code == 401 && reply.get("items").is_none(),
                "{name}: {code} {reply}"
            );
        }
        Ok(())
    })
    .await
}

// ac:8v86osn3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_requests_of_a_read_key_write_no_event_and_change_no_revision() -> Result<()> {
    with_scope(async |s| {
        let before = snapshot(s).await?;
        ensure!(before.0 > 0 && !before.1.is_empty(), "{before:?}");
        for body in own_bodies(s)
            .into_iter()
            .chain([json!({"workspace": s.w2})])
        {
            data(s.w, Some(&s.k1), &body).await?;
        }
        other_routes(s).await?;
        bad_credentials(s).await?;
        let after = snapshot(s).await?;
        ensure!(before == after, "before {before:?}, after {after:?}");
        Ok(())
    })
    .await
}
