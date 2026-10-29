// Part 2 of tests/timeline_http.rs, included there, so the test names stay unchanged.

/// The ac1 fixture: the tree before p, then the five changes after p.
/// Returns p and the report id.
async fn five_changes(w: &World) -> Result<(i64, i64)> {
    w.save("bot", "domain:d", 0, None, "Add the domain").await?;
    w.save(
        "bot",
        "capability:c1",
        0,
        Some("domain:d"),
        "Add the first capability",
    )
    .await?;
    w.save(
        "bot",
        "capability:c2",
        0,
        Some("domain:d"),
        "Add the second capability",
    )
    .await?;
    w.save(
        "bot",
        "requirement:q",
        0,
        Some("capability:c1"),
        "Add the requirement of A",
    )
    .await?;
    w.save("bot", "ac:a", 0, Some("requirement:q"), "Add criterion A")
        .await?;
    w.save("bot", "signal:n", 0, None, "Add signal N").await?;
    let a = w.get("pat", "/objects?address=ac:a").await?;
    w.command(
        "pat",
        json!({"op":"confirm","req":a["process"]["req"],"revision":a["process"]["assessment"]["revision"]}),
    )
    .await?;
    let p = w.head().await?;
    w.save(
        "bot",
        "requirement:r",
        0,
        Some("capability:c1"),
        "Add requirement R",
    )
    .await?;
    w.save(
        "pat",
        "requirement:r",
        1,
        Some("capability:c2"),
        "Move R to the second capability",
    )
    .await?;
    w.command("pat", json!({"op":"archive_object","target":"signal:n","revision":1,"reason":"Signal N is handled"})).await?;
    let a = w.get("pat", "/objects?address=ac:a").await?;
    w.command("pat", json!({"op":"assess","req":a["process"]["req"],"revision":a["process"]["assessment"]["revision"],"ac":a["process"]["criterion"],
        "assessment":{"ready":true,"reason":"The scope of A is known"}})).await?;
    let report = w.record("runner", "0123456789abcdef0123").await?;
    Ok((p, report["record"].as_i64().unwrap_or_default()))
}

// workspace-timeline-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_reads_the_five_changes_after_a_position_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let (p, report) = five_changes(&w).await?;
    let http = w.get("bot", &format!("/timeline?after={p}")).await?;
    let mcp = w
        .mcp(
            w.token("bot"),
            "joppa_timeline",
            json!({"workspace":w.ws,"after":p}),
        )
        .await?;
    ensure!(mcp_shape::timeline(&http) == mcp, "HTTP {http}\nMCP {mcp}");
    ensure!(http["next_cursor"].is_null(), "{http}");
    let items = http["items"].as_array().unwrap();
    ensure!(items.len() == 5, "{http}");
    let mut last = p;
    for item in items {
        for field in ["position", "at", "actor", "address", "change", "reason"] {
            ensure!(
                item.get(field).is_some_and(|v| !v.is_null() && v != ""),
                "{field} is missing: {item}"
            );
        }
        let position = item["position"].as_i64().unwrap();
        ensure!(position > last, "{http}");
        last = position;
    }
    let column = |field: &str| items.iter().map(|i| i[field].clone()).collect::<Vec<_>>();
    ensure!(
        column("change") == ["save", "move", "archive", "readiness", "results"],
        "{http}"
    );
    ensure!(
        column("actor") == ["bot", "pat", "pat", "pat", "runner"],
        "{http}"
    );
    ensure!(
        column("actor_type") == ["agent", "person", "person", "person", "agent"],
        "{http}"
    );
    ensure!(
        column("address")
            == [
                json!("requirement:r"),
                json!("requirement:r"),
                json!("signal:n"),
                json!("ac:a"),
                json!(format!("record/{report}"))
            ],
        "{http}"
    );
    ensure!(
        column("reason")[..4]
            == [
                "Add requirement R",
                "Move R to the second capability",
                "Signal N is handled",
                "The scope of A is known"
            ],
        "{http}"
    );
    ensure!(
        items[4]["reason"]
            .as_str()
            .is_some_and(|r| r == "Test results of commit 0123456789ab: 1 tests"),
        "{http}"
    );
    ensure!(column("revision")[..3] == [1, 2, 2], "{http}");
    // The journal holds exactly these five changes after p.
    ensure!(w.head().await? == last, "{http}");
    Ok(())
}

// workspace-timeline-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_reads_the_timeline_and_a_stranger_gets_the_absent_workspace_reply() -> Result<()>
{
    let w = world().await?;
    // The owner changes W. maria is a member of W, and eve is not.
    w.save("owner", "domain:d", 0, None, "Add the domain")
        .await?;
    let maria = w.token("maria").to_owned();
    let read = |ws: String, auth: (&'static str, String)| {
        let w = &w;
        async move {
            let reply = w
                .http
                .get(format!("{}/api/workspaces/{ws}/timeline?after=0", w.root))
                .header(auth.0, auth.1)
                .send()
                .await?;
            Ok::<_, anyhow::Error>((reply.status().as_u16(), reply.text().await?))
        }
    };
    let (code, body) = read(w.ws.clone(), ("authorization", format!("Bearer {maria}"))).await?;
    let timeline: Value = serde_json::from_str(&body)?;
    ensure!(
        code == 200
            && timeline["items"]
                .as_array()
                .is_some_and(|items| items.iter().any(|i| i["address"] == "domain:d")),
        "{code} {body}"
    );
    let mcp = w
        .mcp(&maria, "joppa_timeline", json!({"workspace":w.ws}))
        .await?;
    ensure!(mcp == mcp_shape::timeline(&timeline), "{mcp} / {timeline}");
    let eve = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&w.pool, &eve, "eve", None).await?;
    let cookie = ("cookie", format!("joppa_session={eve}"));
    let existing = read(w.ws.clone(), cookie.clone()).await?;
    let absent = read(format!("absent-{}", uuid::Uuid::new_v4().simple()), cookie).await?;
    ensure!(
        existing.0 == 404 && existing.1 == r#"{"error":"Workspace not found"}"#,
        "{existing:?}"
    );
    ensure!(existing == absent, "{existing:?} vs {absent:?}");
    Ok(())
}
