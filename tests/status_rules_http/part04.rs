// Part 4 of tests/status_rules_http.rs, included there, so the test names stay unchanged.

// work-priority-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn work_priority_ac1_setting_the_priority_resets_nothing() -> Result<()> {
    let w = world().await?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    w.save("area", "d", None).await?;
    w.save("rule", "r", Some("area:d")).await?;
    w.save("check", "a", Some("rule:r")).await?;
    w.bind("check:a", &["a"]).await?;
    w.report(&[("a", "pass")]).await?;
    w.ready("check:a").await?;
    let before = w.object("check:a").await?;
    ensure!(
        before["process"]["priority"] == "p2",
        "{}",
        before["process"]
    );
    let (code, reply) = w.set_priority(&w.alex, "check:a", "p0").await?;
    ensure!(code == 200, "{code} {reply}");
    let http: Value = w
        .http
        .get(format!("{}/objects?address=check:a", w.base))
        .bearer_auth(&w.bot)
        .send()
        .await?
        .json()
        .await?;
    let mcp = w
        .mcp(&w.bot, "joppa_objects", json!({"address":"check:a"}))
        .await?;
    for read in [&http, &mcp] {
        ensure!(
            read["process"]["priority"] == "p0"
                && read["process"]["assessment"]["state"] == "ready"
                && read["current_revision"] == before["current_revision"],
            "{read}"
        );
    }
    let tests: Value = w
        .http
        .get(format!("{}/tests?ac=check:a", w.base))
        .bearer_auth(&w.bot)
        .send()
        .await?
        .json()
        .await?;
    ensure!(tests["status"] == "green", "{tests}");
    Ok(())
}

// work-priority-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn work_priority_ac2_work_sorts_by_the_priority_chain_from_the_top() -> Result<()> {
    let w = world().await?;
    let task = w.rules_schema(json!(["red", "grey", "green"])).await?;
    let task = task.as_str().context("task kind")?;
    w.save("area", "d1", None).await?;
    w.save("area", "d2", None).await?;
    w.save("rule", "r1", Some("area:d1")).await?;
    w.save("rule", "r2", Some("area:d2")).await?;
    w.save("check", "a1", Some("rule:r1")).await?;
    w.save("check", "a2", Some("rule:r2")).await?;
    w.save("check", "a3", Some("rule:r2")).await?;
    for criterion in ["check:a1", "check:a2", "check:a3"] {
        w.ready(criterion).await?;
    }
    for (target, value) in [
        ("area:d1", "p1"),
        ("area:d2", "p0"),
        ("check:a1", "p0"),
        ("check:a2", "p2"),
        ("check:a3", "p0"),
    ] {
        let (code, reply) = w.set_priority(&w.alex, target, value).await?;
        ensure!(code == 200, "{target}: {code} {reply}");
    }
    let list = w.unstarted("check:").await?;
    ensure!(list == ["check:a3", "check:a2", "check:a1"], "{list:?}");
    // Fixture 2: one open Task under each criterion.
    for (id, parent) in [("t1", "check:a1"), ("t2", "check:a2"), ("t3", "check:a3")] {
        w.command(&w.owner, json!({"op":"save_object","kind":task,"id":id,"revision":0,"parent":parent,"data":{"title":id,"owner":"owner","contract":"Implement and check the contract","purpose":"implementation"},"reason":"Create work"})).await?;
    }
    let available = w
        .mcp(
            &w.bot,
            "joppa_tasks",
            json!({"view":"available","limit":100}),
        )
        .await?;
    let order: Vec<&str> = available["items"]
        .as_array()
        .context("tasks")?
        .iter()
        .filter_map(|t| t["address"].as_str())
        .collect();
    let expected: Vec<String> = ["t3", "t2", "t1"]
        .iter()
        .map(|id| format!("{task}:{id}"))
        .collect();
    ensure!(order == expected, "{order:?}");
    w.command(&w.bot, json!({"op":"claim_next_task"})).await?;
    let t3 = w.object(&format!("{task}:t3")).await?;
    ensure!(t3["process"]["assignee"] == "bot", "{}", t3["process"]);
    Ok(())
}

// work-priority-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn work_priority_ac3_only_a_value_of_the_configured_set_is_a_priority() -> Result<()> {
    let w = world().await?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    w.save("area", "d", None).await?;
    w.save("rule", "r", Some("area:d")).await?;
    let (code, reply) = w.set_priority(&w.alex, "rule:r", "high").await?;
    ensure!(
        code != 200
            && reply["error"]
                .as_str()
                .is_some_and(|e| e.contains("priority is one of p0, p1, p2")),
        "{code} {reply}"
    );
    ensure!(w.object("rule:r").await?["process"]["priority"] == "p2");
    let (code, reply) = w.set_priority(&w.alex, "rule:r", "p1").await?;
    ensure!(code == 200, "{code} {reply}");
    let mcp = w
        .mcp(&w.alex, "joppa_objects", json!({"address":"rule:r"}))
        .await?;
    ensure!(mcp["process"]["priority"] == "p1", "{}", mcp["process"]);
    Ok(())
}

// work-priority-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn work_priority_ac6_the_owner_sets_the_list_of_priority_values() -> Result<()> {
    let w = world().await?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    let tree = w.tree().await?;
    w.command(&w.owner, json!({"op":"set_tree_schema","revision":tree["current_revision"],"types":tree["schema"]["types"],"priorities":["urgent","normal","later"],"reason":"Use our priority words"})).await?;
    ensure!(w.tree().await?["schema"]["priorities"] == json!(["urgent", "normal", "later"]));
    w.save("area", "d", None).await?;
    w.save("rule", "r", Some("area:d")).await?;
    w.save("check", "a1", Some("rule:r")).await?;
    w.save("check", "a2", Some("rule:r")).await?;
    w.ready("check:a1").await?;
    w.ready("check:a2").await?;
    let (code, reply) = w.set_priority(&w.bot, "check:a2", "urgent").await?;
    ensure!(code == 200, "{code} {reply}");
    let (code, reply) = w.set_priority(&w.bot, "check:a1", "p0").await?;
    ensure!(
        code != 200
            && reply["error"]
                .as_str()
                .is_some_and(|e| e.contains("priority is one of urgent, normal, later")),
        "{code} {reply}"
    );
    ensure!(w.object("check:a2").await?["process"]["priority"] == "urgent");
    ensure!(w.object("check:a1").await?["process"]["priority"] == "later");
    let list = w.unstarted("check:").await?;
    ensure!(list == ["check:a2", "check:a1"], "{list:?}");
    Ok(())
}

// dependencies-before-priority-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn dependencies_before_priority_ac1_a_p0_requirement_waits_for_the_p2_one_it_depends_on()
-> Result<()> {
    let w = world().await?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    w.save("area", "c", None).await?;
    w.save("rule", "a", Some("area:c")).await?;
    w.save("rule", "b", Some("area:c")).await?;
    w.save("check", "a1", Some("rule:a")).await?;
    w.save("check", "b1", Some("rule:b")).await?;
    let b = w.object("rule:b").await?;
    let data = b["document"]["data"].clone();
    w.command(&w.owner, json!({"op":"save_object","kind":"rule","id":"b","revision":b["current_revision"],"parent":"area:c","data":data,"links":[{"target":"rule:a","relation":"depends_on","blocking":true}],"reason":"B needs A first"})).await?;
    w.ready("check:a1").await?;
    w.ready("check:b1").await?;
    for (target, value) in [("rule:b", "p0"), ("rule:a", "p2")] {
        let (code, reply) = w.set_priority(&w.alex, target, value).await?;
        ensure!(code == 200, "{target}: {code} {reply}");
    }
    let list = w.unstarted("check:").await?;
    ensure!(list == ["check:a1"], "{list:?}");
    let b1 = w
        .mcp(&w.bot, "joppa_objects", json!({"address":"check:b1"}))
        .await?;
    ensure!(
        b1["process"]["readiness_blocker"]["target"] == "rule:a",
        "{}",
        b1["process"]
    );
    // A1 turns green: B1 becomes plannable.
    w.bind("check:a1", &["a1"]).await?;
    w.report(&[("a1", "pass")]).await?;
    let list = w.unstarted("check:").await?;
    ensure!(list == ["check:b1"], "{list:?}");
    let b1 = w
        .mcp(&w.bot, "joppa_objects", json!({"address":"check:b1"}))
        .await?;
    ensure!(
        b1["process"]["readiness_blocker"].is_null(),
        "{}",
        b1["process"]
    );
    Ok(())
}
