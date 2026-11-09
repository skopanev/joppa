// Part 5 of tests/status_rules_http.rs, included there, so the test names stay unchanged.

// type-status-rules-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn type_status_rules_ac6_a_links_node_takes_the_highest_tone_of_its_linking_objects()
-> Result<()> {
    let w = world().await?;
    let task = w.rules_schema().await?;
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
    let related = json!({"related":{"edges":[{"to":"user:u"}]}});
    let save_linked = |kind: &str, id: &str, parent: &str, extra: Value| {
        let mut data = json!({"title":id,"owner":"owner","body":"Given: a node.\nWhen: alex reads it.\nThen: it has a tone.\nEvidence: the tone."});
        for (k, v) in extra.as_object().into_iter().flatten() {
            data[k] = v.clone();
        }
        json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"relations":related,"reason":"Link to the user"})
    };
    // Fixture 1: no object links to U, so U is orange.
    ensure!(w.tones(&["user:u"]).await? == ["orange"], "fixture 1");
    // Fixture 2: green check A links to U.
    w.command(&w.owner, save_linked("check", "a", "rule:r", json!({})))
        .await?;
    w.bind("check:a", &["a"]).await?;
    w.report(&[("a", "pass")]).await?;
    ensure!(
        w.tones(&["check:a", "user:u"]).await? == ["green", "green"],
        "fixture 2"
    );
    // Fixture 3: a Task under A links to U.
    w.command(
        &w.owner,
        save_linked(
            task,
            "t",
            "check:a",
            json!({"body":"Implement and check the contract"}),
        ),
    )
    .await?;
    // A Task has the rule none: U does not count it, and its archive changes nothing.
    ensure!(w.tones(&["user:u"]).await? == ["green"], "fixture 3");
    w.archive(&format!("{task}:t")).await?;
    ensure!(w.tones(&["user:u"]).await? == ["green"], "fixture 3 archived");
    // Fixture 4: red check B links to U.
    w.command(&w.owner, save_linked("check", "b", "rule:r", json!({})))
        .await?;
    w.bind("check:b", &["b"]).await?;
    w.report(&[("a", "pass"), ("b", "fail")]).await?;
    ensure!(w.tones(&["user:u"]).await? == ["red"], "fixture 4");
    // Fixture 5: B is archived and an open Task links to U. A Task has the rule none, so
    // U does not count it and stays green.
    w.archive("check:b").await?;
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
            json!({"body":"Implement the next part"}),
        ),
    )
    .await?;
    ensure!(w.tones(&["user:u"]).await? == ["green"], "fixture 5");
    // Fixture 6: the open Task is archived, and an archived Task does not count.
    w.archive(&format!("{task}:open")).await?;
    ensure!(w.tones(&["user:u"]).await? == ["green"], "fixture 6");
    Ok(())
}
