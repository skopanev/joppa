// Part 5 of tests/status_rules_http.rs, included there, so the test names stay unchanged.

// ready-signal-takes-tasks-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ready_signal_takes_tasks_ac1_a_task_under_a_signal_waits_for_its_ready_mark() -> Result<()>
{
    let w = world().await?;
    let tree = w.tree().await?;
    let mut types = tree["schema"]["types"].clone();
    let kind_of = |role: &str, inbox: bool| -> Result<String> {
        types
            .as_object()
            .and_then(|t| {
                t.iter()
                    .find(|(_, v)| v["role"] == role && (!inbox || v["inbox"] == true))
            })
            .map(|(k, _)| k.clone())
            .with_context(|| format!("{role} type"))
    };
    let (task, signal) = (kind_of("task", false)?, kind_of("document", true)?);
    types[&task]["parents"]
        .as_array_mut()
        .context("task parents")?
        .push(json!(signal));
    w.command(&w.owner, json!({"op":"set_tree_schema","revision":tree["current_revision"],"types":types,"reason":"Allow work under a signal"})).await?;
    w.command(&w.owner, json!({"op":"save_object","kind":signal,"id":"n","revision":0,"parent":null,"data":{"title":"Login fails","owner":"owner","source":"support chat","body":"The login page fails"},"reason":"Record the signal"})).await?;
    let n = format!("{signal}:n");
    let t = format!("{task}:t");
    w.command(&w.owner, json!({"op":"save_object","kind":task,"id":"t","revision":0,"parent":n,"data":{"title":"Fix the login","owner":"owner","body":"Fix the login page and check it","purpose":"implementation"},"reason":"Create work"})).await?;
    let revision = w.object(&t).await?["current_revision"].clone();
    // The first claim is refused: N has no readiness mark.
    let (code, reply) = w
        .try_command(
            &w.bot,
            json!({"op":"claim_task","id":t,"revision":revision}),
        )
        .await?;
    ensure!(
        code != 200 && reply["error"] == format!("{n} has no readiness assessment"),
        "{code} {reply}"
    );
    ensure!(w.object(&t).await?["process"]["assignee"].is_null());
    // Member P marks N ready for work; the second claim succeeds.
    w.command(
        &w.alex,
        json!({"op":"assess_signal","target":n,"ready":true,"reason":"Clear fix"}),
    )
    .await?;
    let revision = w.object(&t).await?["current_revision"].clone();
    w.command(
        &w.bot,
        json!({"op":"claim_task","id":t,"revision":revision}),
    )
    .await?;
    let claimed = w.object(&t).await?;
    ensure!(
        claimed["process"]["status"] == "in_progress" && claimed["process"]["assignee"] == "bot",
        "{}",
        claimed["process"]
    );
    let read = w.mcp(&w.bot, "joppa_objects", json!({"address":n})).await?;
    ensure!(
        read["process"]["assessment"]["state"] == "ready"
            && read["process"]["assessment"]["reason"] == "Clear fix",
        "{read}"
    );
    let revision = w.object(&t).await?["current_revision"].clone();
    w.command(
        &w.bot,
        json!({"op":"complete_task","commit":"c0ffee0","id":t,"revision":revision,"evidence":"The login page works"}),
    )
    .await?;
    let done = w.object(&t).await?;
    ensure!(
        done["process"]["status"] == "done" && done["process"].get("verification").is_none(),
        "{}",
        done["process"]
    );
    Ok(())
}

// work-priority-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn work_priority_ac5_a_member_sets_the_priority_and_the_timeline_names_it() -> Result<()> {
    // alex is a member with write access and is not the owner.
    let w = world().await?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    w.save("area", "d", None).await?;
    w.save("rule", "r", Some("area:d")).await?;
    w.save("check", "a", Some("rule:r")).await?;
    w.ready("check:a").await?;
    ensure!(w.object("check:a").await?["process"]["priority"] == "p2");
    let before: Value = w
        .http
        .get(format!("{}/timeline?order=newest&limit=1", w.base))
        .bearer_auth(&w.alex)
        .send()
        .await?
        .json()
        .await?;
    let after = before["items"][0]["position"]
        .as_u64()
        .with_context(|| format!("timeline: {before}"))?;
    let (code, reply) = w.set_priority(&w.alex, "check:a", "p0").await?;
    ensure!(code == 200, "{code} {reply}");
    let read = w
        .mcp(&w.alex, "joppa_objects", json!({"address":"check:a"}))
        .await?;
    ensure!(read["process"]["priority"] == "p0", "{}", read["process"]);
    let timeline: Value = w
        .http
        .get(format!("{}/timeline?after={after}", w.base))
        .bearer_auth(&w.alex)
        .send()
        .await?
        .json()
        .await?;
    let entries = timeline["items"].as_array().context("items")?;
    ensure!(entries.len() == 1, "{timeline}");
    let entry = &entries[0];
    ensure!(
        entry["actor"] == "alex"
            && entry["actor_type"] == "person"
            && entry["change"] == "priority"
            && entry["address"] == "check:a",
        "{entry}"
    );
    Ok(())
}

// type-status-rules-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn type_status_rules_ac6_a_links_node_takes_the_first_status_that_a_linking_object_has()
-> Result<()> {
    let w = world().await?;
    let task = w.rules_schema(json!(["red", "grey", "green"])).await?;
    let task = task.as_str().context("task kind")?;
    let tree = w.tree().await?;
    let mut types = tree["schema"]["types"].clone();
    types["user"] =
        json!({"title":"Users","role":"document","root":true,"parents":[],"status":"links"});
    w.command(&w.owner, json!({"op":"set_tree_schema","revision":tree["current_revision"],"types":types,"reason":"Add users with a links rule"})).await?;
    ensure!(w.tree().await?["schema"]["types"]["user"]["status"] == "links");
    w.save("user", "u", None).await?;
    w.save("area", "g", None).await?;
    w.save("rule", "r", Some("area:g")).await?;
    let link = json!([{"target":"user:u","relation":"related","blocking":false}]);
    let save_linked = |kind: &str, id: &str, parent: &str, extra: Value| {
        let mut data = json!({"title":id,"owner":"owner","body":"The contract","body":"Given: a node.\nWhen: alex reads it.\nThen: it has a tone.\nEvidence: the tone."});
        for (k, v) in extra.as_object().into_iter().flatten() {
            data[k] = v.clone();
        }
        json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"links":link,"reason":"Link to the user"})
    };
    // Fixture 1: no object links to U, so U is grey.
    ensure!(w.tones(&["user:u"]).await? == ["grey"], "fixture 1");
    // Fixture 2: green check A links to U.
    w.command(&w.owner, save_linked("check", "a", "rule:r", json!({})))
        .await?;
    w.bind("check:a", &["a"]).await?;
    w.report(&[("a", "pass")]).await?;
    ensure!(
        w.tones(&["check:a", "user:u"]).await? == ["green", "green"],
        "fixture 2"
    );
    // Fixture 3: a done Task under A links to U and takes the status of A.
    w.command(
        &w.owner,
        save_linked(
            task,
            "t",
            "check:a",
            json!({"body":"Implement and check the contract","purpose":"implementation"}),
        ),
    )
    .await?;
    let p = w.object("check:a").await?["process"].clone();
    w.command(&w.owner, json!({"op":"assess","req":p["req"],"revision":p["revision"],"ac":p["criterion"],"assessment":{"ready":true,"reason":"Known scope","recipient":null,"decision":null}})).await?;
    let t = format!("{task}:t");
    let revision = w.object(&t).await?["current_revision"].clone();
    w.command(
        &w.bot,
        json!({"op":"claim_task","id":t,"revision":revision}),
    )
    .await?;
    let revision = w.object(&t).await?["current_revision"].clone();
    w.command(
        &w.bot,
        json!({"op":"complete_task","commit":"c0ffee0","id":t,"revision":revision,"evidence":"Implemented"}),
    )
    .await?;
    ensure!(w.tones(&["user:u"]).await? == ["green"], "fixture 3");
    // Fixture 4: red check B links to U.
    w.command(&w.owner, save_linked("check", "b", "rule:r", json!({})))
        .await?;
    w.bind("check:b", &["b"]).await?;
    w.report(&[("a", "pass"), ("b", "fail")]).await?;
    ensure!(w.tones(&["user:u"]).await? == ["red"], "fixture 4");
    // Fixture 5: B is archived and an open Task links to U, so U is grey.
    let b = w.object("check:b").await?;
    w.command(&w.owner, json!({"op":"archive_object","target":"check:b","revision":b["current_revision"],"reason":"Drop the check"})).await?;
    w.report(&[("a", "pass")]).await?;
    ensure!(
        w.tones(&["user:u"]).await? == ["green"],
        "after the archive"
    );
    w.command(
        &w.owner,
        save_linked(
            task,
            "open",
            "check:a",
            json!({"body":"Implement the next part","purpose":"implementation"}),
        ),
    )
    .await?;
    ensure!(w.tones(&["user:u"]).await? == ["grey"], "fixture 5");
    // Fixture 6: the open Task is retired, and a retired Task does not count.
    let o = format!("{task}:open");
    let revision = w.object(&o).await?["current_revision"].clone();
    w.command(
        &w.owner,
        json!({"op":"retire_task","id":o,"revision":revision,"reason":"Not needed"}),
    )
    .await?;
    ensure!(w.tones(&["user:u"]).await? == ["green"], "fixture 6");
    Ok(())
}
