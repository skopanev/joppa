// Part 2 of tests/readonly_mcp_http.rs: HTTP write refusals, MCP/agent role isolation and
// the last-Editor rule.

// ac:lb3j2on1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn viewer_writes_nothing_over_http_including_prior_upload_link() -> Result<()> {
    let fake = fake_spaces::start();
    fake.configure();
    let w = record_world::world_env(&["tests/x.rs > t"], &fake.env()).await?;
    // alex starts as an Editor ("human") with service token `w.alex`, a browser session and an
    // MCP OAuth token. alex gets a signed upload_url before owner demotes alex to Read-only.
    let alex_cookie = browser_session(&w.pool, "alex").await?;
    let alex_mcp = oauth_token(&w, &alex_cookie).await?;
    let owner_cookie = browser_session(&w.pool, "owner").await?;
    let link = w
        .mcp(
            &alex_mcp,
            "joppa_attach",
            json!({"target":"ac:a","filename":"late.txt","upload":true}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
    let upload_url = link["upload_url"].as_str().context("upload_url")?.to_owned();

    let demote = w
        .http
        .post(format!("{}/api/access/role", w.root))
        .header("Origin", &w.root)
        .header("Cookie", &owner_cookie)
        .json(&json!({"workspace":w.ws,"member":"alex","role":"viewer"}))
        .send()
        .await?;
    ensure!(demote.status() == 200, "{}", demote.text().await?);

    let before_pos: i64 = w
        .pool
        .get()
        .await?
        .query_one(
            "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);

    let put = w
        .http
        .put(&upload_url)
        .body("late upload bytes")
        .send()
        .await?;
    ensure!(put.status() == 403, "PUT status {}", put.status());
    let put_body: Value = put.json().await?;
    ensure!(put_body == json!({"error":"a Read-only member cannot write"}));

    let checks = [
        (
            format!("/api/workspaces/{}/commands", w.ws),
            Some(json!({"request_id":"h1","command":{"op":"note","target":"ac:a","id":"n1","text":"hi"}})),
        ),
        (
            format!("/api/workspaces/{}/order", w.ws),
            Some(json!({"request_id":"h2","kind":"requirement","parent":null,"expected":["r"],"ids":["r"]})),
        ),
        (format!("/api/workspaces/{}/dependencies", w.ws), None),
        (format!("/api/workspaces/{}/blobs", w.ws), None),
    ];
    for (path, body) in checks {
        let url = format!("{}{path}", w.root);
        let req = match body {
            Some(v) => w.http.post(url).bearer_auth(&w.alex).json(&v),
            None => w.http.post(url).bearer_auth(&w.alex).body("raw"),
        };
        let res = req.send().await?;
        ensure!(res.status() == 403, "{path}: {}", res.status());
        let payload: Value = res.json().await?;
        ensure!(
            payload == json!({"error":"a Read-only member cannot write"}),
            "{path}: {payload}"
        );
    }
    ensure!(
        !fake
            .keys()
            .iter()
            .any(|k| k.starts_with(&format!("workspaces/{}/", w.ws))),
        "Spaces got a file"
    );
    let after_pos: i64 = w
        .pool
        .get()
        .await?
        .query_one(
            "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(after_pos == before_pos);
    Ok(())
}

// ac:lg5zo4gu
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn mcp_and_agents_never_change_a_member_or_a_role() -> Result<()> {
    let w = record_world::world_env(&["tests/x.rs > t"], &[]).await?;
    let (maria, maria_cookie, _) = enroll_viewer_maria(&w.pool, &w.ws).await?;
    let owner_cookie = browser_session(&w.pool, "owner").await?;
    let owner_mcp = oauth_token(&w, &owner_cookie).await?;

    let tools = mcp_rpc(&w, &owner_mcp, "tools/list", json!({})).await?;
    let serialized = tools.to_string();
    for hidden in ["\"member\"", "\"remove_member\"", "\"set_role\""] {
        ensure!(
            !serialized.contains(hidden),
            "tools/list leaked {hidden}: {serialized}"
        );
    }
    for op in [
        json!({"op":"member","id":"intruder","role":"viewer"}),
        json!({"op":"set_role","id":maria,"role":"human"}),
    ] {
        let refused = w
            .mcp(
                &owner_mcp,
                "joppa_command",
                json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":op}),
            )
            .await?;
        ensure!(refused.is_err(), "{op} succeeded over MCP");
    }

    let before = store::list_owned_access(&w.pool, "owner").await?;

    // Agent bot sends POST /api/access/role with its service credential -> 401.
    let bot_res = w
        .http
        .post(format!("{}/api/access/role", w.root))
        .header("Origin", &w.root)
        .bearer_auth(&w.bot)
        .json(&json!({"workspace":w.ws,"member":maria,"role":"human"}))
        .send()
        .await?;
    ensure!(bot_res.status() == 401, "bot status {}", bot_res.status());
    let bot_body: Value = bot_res.json().await?;
    ensure!(bot_body == json!({"error":"Sign in with the browser to change a role"}));

    // Viewer maria sends POST /api/access/role from a signed-in browser -> refused.
    let maria_res = w
        .http
        .post(format!("{}/api/access/role", w.root))
        .header("Origin", &w.root)
        .header("Cookie", &maria_cookie)
        .json(&json!({"workspace":w.ws,"member":maria,"role":"human"}))
        .send()
        .await?;
    ensure!(!maria_res.status().is_success());
    let maria_body: Value = maria_res.json().await?;
    ensure!(maria_body == json!({"error":"a human member of the workspace is required"}));

    // Editor owner sends POST /api/access/role with foreign Origin -> 403.
    let csrf_res = w
        .http
        .post(format!("{}/api/access/role", w.root))
        .header("Origin", "https://attacker.example")
        .header("Cookie", &owner_cookie)
        .json(&json!({"workspace":w.ws,"member":maria,"role":"human"}))
        .send()
        .await?;
    ensure!(csrf_res.status() == 403, "csrf status {}", csrf_res.status());

    let after = store::list_owned_access(&w.pool, "owner").await?;
    ensure!(after == before, "access changed: {after}");
    Ok(())
}

// ac:twi9vco1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_last_editor_stays_an_editor() -> Result<()> {
    let w = record_world::world(&["tests/x.rs > t"]).await?;
    let solo = format!("solo-{}", uuid::Uuid::new_v4().simple());
    let create: Operation = serde_json::from_value(json!({"op":"workspace","title":"Solo"}))?;
    store::execute(&w.pool, &solo, "owner", "create-solo", &create).await?;
    let (maria, _, _) = enroll_viewer_maria(&w.pool, &solo).await?;
    let owner_cookie = browser_session(&w.pool, "owner").await?;

    let demote = w
        .http
        .post(format!("{}/api/access/role", w.root))
        .header("Origin", &w.root)
        .header("Cookie", &owner_cookie)
        .json(&json!({"workspace":solo,"member":"owner","role":"viewer"}))
        .send()
        .await?;
    ensure!(!demote.status().is_success());
    let demote_body: Value = demote.json().await?;
    ensure!(demote_body == json!({"error":"the last human member cannot become Read-only"}));

    let remove = w
        .http
        .post(format!("{}/api/access/remove", w.root))
        .header("Origin", &w.root)
        .header("Cookie", &owner_cookie)
        .json(&json!({"workspace":solo,"member":"owner"}))
        .send()
        .await?;
    ensure!(!remove.status().is_success());
    let remove_body: Value = remove.json().await?;
    ensure!(remove_body == json!({"error":"the last human member cannot be removed"}));

    // Promote maria to Editor, then demote owner to Read-only.
    let promote = w
        .http
        .post(format!("{}/api/access/role", w.root))
        .header("Origin", &w.root)
        .header("Cookie", &owner_cookie)
        .json(&json!({"workspace":solo,"member":maria,"role":"human"}))
        .send()
        .await?;
    ensure!(promote.status() == 200, "{}", promote.text().await?);
    let step_down = w
        .http
        .post(format!("{}/api/access/role", w.root))
        .header("Origin", &w.root)
        .header("Cookie", &owner_cookie)
        .json(&json!({"workspace":solo,"member":"owner","role":"viewer"}))
        .send()
        .await?;
    ensure!(step_down.status() == 200, "{}", step_down.text().await?);

    let full = store::read(&w.pool, &solo, &maria, &store::Scope::full()).await?;
    ensure!(
        full["state"]["members"] == json!({"owner":"viewer", maria:"human"}),
        "{}",
        full["state"]["members"]
    );
    Ok(())
}
