// Part 32 of tests/reports_http.rs, included there, so the test names stay unchanged.

// dependencies-before-priority-ac7
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac7_an_ordering_link_greys_nothing_and_an_archived_target_frees_work() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:d", &["criterion:a1"]).await?;
    p.bind(&w.token, "criterion:b1", &[A]).await?;
    // check-list-frozen-ac3: the list save clears readiness, so B1 is assessed after it.
    let ready = json!({"ready":true,"reason":"Known scope"});
    p.assess("spec:b", &[("criterion:b1", ready)]).await?;
    p.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(p.status("criterion:b1").await?["status"] == "green");
    ensure!(p.status("criterion:a1").await?["status"] == "grey");
    let readiness = p.readiness("criterion:b1").await?;
    ensure!(readiness[0] == "ready", "{readiness}");
    let before = p.saved().await?["position"].as_i64().unwrap();

    // P adds a blocking link from B to A without a revision.
    p.link("spec:b", ordering_link("spec:a")).await?;
    ensure!(p.status("criterion:b1").await?["status"] == "green");
    ensure!(p.readiness("criterion:b1").await? == readiness);
    let b = p.read("spec:b").await?;
    let stored = &b["document"]["links"][0];
    ensure!(
        stored["target"] == "spec:a"
            && stored["blocking"] == true
            && stored.get("revision").is_none(),
        "{b}"
    );
    let events = p.saved().await?["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["position"].as_i64() > Some(before))
        .cloned()
        .collect::<Vec<_>>();
    ensure!(
        events.len() == 1
            && events[0]["actor"] == "owner"
            && events[0]["command"]["op"] == "save_object"
            && events[0]["command"]["kind"] == "spec"
            && events[0]["command"]["id"] == "b"
            && events[0]["command"]["reason"] == "Order the work"
            && events[0]["command"]["links"][0].get("revision").is_none(),
        "{events:?}"
    );
    // The timeline reader is absent from this checkout, so its HTTP and MCP check is pending.

    let b1 = p.object_both("criterion:b1").await?;
    ensure!(waits_for(&b1, "spec:a"), "{b1}");
    // P archives A1, so A has no live criterion and still holds B1. Then P archives A.
    for (target, held) in [("criterion:a1", true), ("spec:a", false)] {
        let revision = p.revision(target).await?;
        p.owner_command(json!({"op":"archive_object","target":target,"revision":revision,"reason":"Retire the scope"}))
            .await?;
        let b1 = p.object_both("criterion:b1").await?;
        let free = b1["work_ready"] == true && b1.get("waits_for").is_none();
        ensure!(
            if held { waits_for(&b1, "spec:a") } else { free },
            "after archiving {target}: {b1}"
        );
    }

    // A text change of B is a change of B. A link to the archived A cannot be saved again.
    let object = p.read("spec:b").await?;
    p.owner_command(json!({"op":"save_object","kind":"spec","id":"b","revision":object["current_revision"],"parent":"cap:c",
        "data":{"title":"spec:b","body":"Deliver the sharper rule","owner":"owner"},"links":[],"reason":"Sharpen the rule"}))
        .await?;
    ensure!(p.status("criterion:b1").await?["status"] == "grey");
    store::check_journal(&p.pool).await?;
    Ok(())
}

// dependencies-before-priority-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac5_the_dependency_route_refuses_foreign_origin_non_members_and_cookie_commands()
-> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1"]).await?;
    let root = w.base.split("/api/").next().context("root")?.to_owned();
    let route = format!("{root}/api/workspaces/{}", p.ws);
    let session = |actor: &'static str| {
        let pool = p.pool.clone();
        async move {
            let session = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            sessions::create(&pool, &session, actor, None).await?;
            anyhow::Ok(session)
        }
    };
    let send = |session: String, origin: String, path: &'static str, body: Value| {
        let request = w
            .http
            .post(format!("{route}{path}"))
            .header("cookie", format!("joppa_session={session}"))
            .header("origin", origin)
            .json(&body);
        async move {
            let reply = request.send().await?;
            anyhow::Ok((
                reply.status().as_u16(),
                reply.json::<Value>().await.unwrap_or(Value::Null),
            ))
        }
    };
    let add = || json!({"request_id":uuid::Uuid::new_v4().to_string(),"source":"spec:b","target":"spec:a","action":"add"});
    let member = session("owner").await?;
    let outsider = session("outsider").await?;
    let b = p.read("spec:b").await?;
    let before = p.saved().await?;
    let refusals = [
        (
            "foreign Origin",
            send(
                member.clone(),
                "http://evil.example".into(),
                "/dependencies",
                add(),
            )
            .await?,
            403,
        ),
        (
            "non-member cookie session",
            send(outsider, root.clone(), "/dependencies", add()).await?,
            403,
        ),
        (
            "member cookie dependencies",
            send(member.clone(), root.clone(), "/dependencies", add()).await?,
            403,
        ),
        (
            "cookie commands",
            send(member.clone(), root.clone(), "/commands",
                json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"save_object","kind":"spec","id":"b",
                    "revision":b["current_revision"],"parent":"cap:c","data":b["document"]["data"],
                    "links":ordering_link("spec:a"),"reason":"Cookie write"}})).await?,
            403,
        ),
    ];
    for (case, (code, reply), expected) in refusals {
        ensure!(code == expected, "{case}: {code} {reply}");
        ensure!(p.saved().await? == before, "{case} changed saved state");
    }
    ensure!(p.read("spec:b").await?["document"]["links"] == json!([]));
    store::check_journal(&p.pool).await?;
    Ok(())
}
