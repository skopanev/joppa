//! A signed-in browser user creates a workspace through the real server.
use anyhow::{Result, ensure};
use joppa::{commands::Command, sessions, store};
use serde_json::{Value, json};
use std::{
    process::{Child, Command as Process, Stdio},
    time::Duration,
};

struct Server(Child, std::path::PathBuf);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

/// Every row that a creation may change. A refused creation leaves it equal.
async fn snapshot(pool: &deadpool_postgres::Pool) -> Result<Vec<String>> {
    let conn = pool.get().await?;
    let mut rows = vec![];
    for table in [
        "workspaces",
        "workspace_aliases",
        "events",
        "state_entries",
        "board_cache",
        "command_receipts",
    ] {
        rows.push(conn.query_one(
            &format!("SELECT COALESCE(string_agg(t::text, E'\\n' ORDER BY t::text), '') FROM joppa.{table} t"),
            &[],
        ).await?.get(0));
    }
    Ok(rows)
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn browser_user_creates_and_owns_a_workspace() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let db: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        db.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let (alex, sam) = (format!("alex-{id}"), format!("sam-{id}"));
    let (base, hidden) = (format!("base-{id}"), format!("hidden-{id}"));
    for workspace in [&base, &hidden] {
        store::execute(
            &pool,
            workspace,
            "owner",
            "create",
            &Command::Workspace {
                title: workspace.clone(),
            },
        )
        .await?;
        for (i, member) in [&alex, &sam].into_iter().enumerate() {
            store::execute(
                &pool,
                workspace,
                "owner",
                &format!("member-{i}"),
                &Command::Member {
                    id: member.clone(),
                    role: "human".into(),
                },
            )
            .await?;
        }
    }
    let dir = std::env::temp_dir().join(format!("joppa-create-{id}"));
    std::fs::create_dir_all(&dir)?;
    let (tokens, rules, users) = (
        dir.join("tokens.json"),
        dir.join("access.json"),
        dir.join("users.json"),
    );
    let alex_key = format!("alex-key-{id}");
    std::fs::write(
        &tokens,
        json!({format!("owner-key-{id}"):"owner", alex_key.clone():alex, format!("sam-key-{id}"):sam})
            .to_string(),
    )?;
    // The rules name only the base workspace. Neither user has a rule for hidden.
    std::fs::write(
        &rules,
        json!({"emails":{"alex@example.test":[base],"sam@example.test":[base]}}).to_string(),
    )?;
    std::fs::write(
        &users,
        json!({"alex@example.test":alex,"sam@example.test":sam}).to_string(),
    )?;
    let (alex_cookie, sam_cookie) = (
        format!("joppa_session={}{}", id, uuid::Uuid::new_v4().simple()),
        format!("joppa_session={}{}", uuid::Uuid::new_v4().simple(), id),
    );
    sessions::create(&pool, alex_cookie.split_once('=').unwrap().1, &alex).await?;
    sessions::create(&pool, sam_cookie.split_once('=').unwrap().1, &sam).await?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let _server = Server(
        Process::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("JOPPA_DATABASE_URL", &url)
            .env("JOPPA_TOKENS_FILE", &tokens)
            .env("JOPPA_LOGIN_USERS_FILE", &users)
            .env("JOPPA_ACCESS_RULES_FILE", &rules)
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("DEEPINFRA_API_KEY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
        dir,
    );
    let root = format!("http://127.0.0.1:{port}");
    let mut ready = false;
    for _ in 0..100 {
        if reqwest::get(format!("{root}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    ensure!(ready, "test server did not start");
    let client = reqwest::Client::new();
    let get = |path: String, cookie: &str| {
        client
            .get(format!("{root}{path}"))
            .header("cookie", cookie)
            .send()
    };
    let create = |body: Value, cookie: Option<&str>, origin: Option<&str>| {
        let mut request = client.post(format!("{root}/api/workspaces")).json(&body);
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        request.send()
    };
    let response = get("/api/workspaces".into(), &alex_cookie).await?;
    assert!(response.status().is_success());
    let list_before = response.json::<Value>().await?;
    assert_eq!(list_before, json!([{"id":base,"title":base}]));
    let unchanged = snapshot(&pool).await?;

    // Step 1: a session, write permission and the same origin are required.
    let response = create(json!({"title":"Billing"}), None, Some(&root)).await?;
    assert_eq!(response.status(), 401);
    assert_eq!(
        response.json::<Value>().await?,
        json!({"error":"Please sign in to continue"})
    );
    for origin in [Some("http://evil.example"), None] {
        let response = create(json!({"title":"Billing"}), Some(&alex_cookie), origin).await?;
        assert_eq!(response.status(), 403, "origin {origin:?}");
    }
    let response = client
        .post(format!("{root}/api/workspaces"))
        .bearer_auth(&alex_key)
        .header("origin", &root)
        .json(&json!({"title":"Billing"}))
        .send()
        .await?;
    assert_eq!(
        response.status(),
        403,
        "a bearer key is not a browser session"
    );
    // Step 2: the client cannot choose either ID.
    for body in [
        json!({"title":"Billing","id":"chosen"}),
        json!({"title":"Billing","request_id":"chosen"}),
    ] {
        let response = create(body, Some(&alex_cookie), Some(&root)).await?;
        assert!(response.status().is_client_error());
    }
    // Step 4: the Workspace command validates the title and the transaction rolls back.
    let response = create(json!({"title":"  "}), Some(&alex_cookie), Some(&root)).await?;
    assert!(response.status().is_client_error());
    assert_eq!(
        response.json::<Value>().await?,
        json!({"error":"text must not be empty"})
    );
    assert_eq!(snapshot(&pool).await?, unchanged);
    let response = get("/api/workspaces".into(), &alex_cookie).await?;
    assert!(response.status().is_success());
    assert_eq!(response.json::<Value>().await?, list_before);

    // Steps 3 and 5: one Workspace operation creates Billing; the reply names it.
    let response = create(json!({"title":"Billing"}), Some(&alex_cookie), Some(&root)).await?;
    assert_eq!(response.status(), 200);
    let created = response.json::<Value>().await?;
    let billing = created["id"].as_str().unwrap().to_owned();
    assert_eq!(created, json!({"id":billing,"title":"Billing"}));
    assert!(billing != base && billing != hidden);
    // The receipt holds the one submitted operation; internal migrations may follow it.
    let receipt = |workspace: String| {
        let pool = pool.clone();
        async move {
            let conn = pool.get().await?;
            let rows = conn
                .query(
                    "SELECT actor,request_id,input::text FROM joppa.command_receipts WHERE workspace=$1",
                    &[&workspace],
                )
                .await?;
            ensure!(rows.len() == 1, "one request per creation");
            let creations: i64 = conn
                .query_one(
                    "SELECT count(*) FROM joppa.events WHERE workspace=$1 AND command->>'op'='workspace'",
                    &[&workspace],
                )
                .await?
                .get(0);
            ensure!(creations == 1, "one workspace event per creation");
            let input: Value = serde_json::from_str(rows[0].get(2))?;
            anyhow::Ok((
                rows[0].get::<_, String>(0),
                rows[0].get::<_, String>(1),
                input,
            ))
        }
    };
    let (actor, request_id, input) = receipt(billing.clone()).await?;
    assert_eq!(actor, alex);
    assert_eq!(
        input,
        json!({"batch":false,"operations":[{"command":{"op":"workspace","title":"Billing"}}]})
    );
    // Step 2: a second call gets new IDs.
    let second = create(json!({"title":"Billing"}), Some(&alex_cookie), Some(&root))
        .await?
        .json::<Value>()
        .await?;
    let second_id = second["id"].as_str().unwrap().to_owned();
    assert_ne!(second_id, billing);
    assert_ne!(receipt(second_id.clone()).await?.1, request_id);

    // Step 6: alex owns Billing although the rules do not name it.
    let list = get("/api/workspaces".into(), &alex_cookie)
        .await?
        .json::<Value>()
        .await?;
    let items = list.as_array().unwrap();
    assert!(items.contains(&json!({"id":billing,"title":"Billing"})));
    assert!(!items.iter().any(|item| item["id"] == hidden));
    let read = get(format!("/api/workspaces/{billing}"), &alex_cookie).await?;
    assert_eq!(read.status(), 200);
    let read = read.json::<Value>().await?;
    // Step 7: the default read names the owner next to the unchanged members.
    assert_eq!(read["index"]["owner"], json!(alex));
    assert_eq!(read["index"]["members"], json!({alex.clone():"human"}));
    assert_eq!(
        get(format!("/api/workspaces/{billing}/board"), &alex_cookie)
            .await?
            .status(),
        200
    );
    let base_read = get(format!("/api/workspaces/{base}"), &alex_cookie)
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(base_read["index"]["owner"], "owner");
    // A member who does not own a workspace that the rules exclude still cannot see it.
    for path in [
        format!("/api/workspaces/{hidden}"),
        format!("/api/workspaces/{hidden}/board"),
    ] {
        assert_eq!(get(path, &alex_cookie).await?.status(), 404);
    }
    // The second signed-in user is not a member of Billing.
    for path in [
        format!("/api/workspaces/{billing}"),
        format!("/api/workspaces/{billing}/board"),
    ] {
        let response = get(path, &sam_cookie).await?;
        assert_eq!(response.status(), 404);
        assert_eq!(
            response.json::<Value>().await?,
            json!({"error":"Workspace not found"})
        );
    }
    let sam_list = get("/api/workspaces".into(), &sam_cookie)
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(sam_list, json!([{"id":base,"title":base}]));
    // Ownership does not replace membership. No public command removes the owner's
    // membership, so the fixture deletes that row from the second owned workspace.
    let removed = pool
        .get()
        .await?
        .execute(
            "DELETE FROM joppa.state_entries WHERE workspace=$1 AND section='members' AND key=$2",
            &[&second_id, &alex],
        )
        .await?;
    assert_eq!(removed, 1);
    let list = get("/api/workspaces".into(), &alex_cookie)
        .await?
        .json::<Value>()
        .await?;
    assert!(
        !list
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["id"] == second_id)
    );
    assert!(list.as_array().unwrap().iter().any(|w| w["id"] == billing));
    for path in [
        format!("/api/workspaces/{second_id}"),
        format!("/api/workspaces/{second_id}/board"),
    ] {
        let response = get(path, &alex_cookie).await?;
        assert_eq!(response.status(), 404);
        assert_eq!(
            response.json::<Value>().await?,
            json!({"error":"Workspace not found"})
        );
    }

    // Step 3: the commands route keeps its refusals.
    let response = client
        .post(format!("{root}/api/workspaces/{billing}/commands"))
        .header("cookie", &alex_cookie)
        .header("origin", &root)
        .json(&json!({"request_id":"again","command":{"op":"workspace","title":"Again"}}))
        .send()
        .await?;
    assert_eq!(response.status(), 403, "a browser session stays read-only");
    let response = client
        .post(format!("{root}/api/workspaces/remote-{id}/commands"))
        .bearer_auth(&alex_key)
        .json(&json!({"request_id":"remote","command":{"op":"workspace","title":"Remote"}}))
        .send()
        .await?;
    assert!(response.status().is_client_error());
    assert!(
        response
            .text()
            .await?
            .contains("workspace creation is available only through the server CLI")
    );

    // Step 4: an ID collision returns the existing error and rolls back.
    let unchanged = snapshot(&pool).await?;
    let error = store::execute(
        &pool,
        &billing,
        &alex,
        "collision",
        &Command::Workspace {
            title: "Billing".into(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "workspace exists");
    assert_eq!(snapshot(&pool).await?, unchanged);
    Ok(())
}
