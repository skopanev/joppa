// Part 3 of tests/status_rules_http.rs, included there, so the test names stay unchanged.

// type-status-rules-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn type_status_rules_ac2_a_node_takes_the_first_status_in_its_order_that_a_child_has()
-> Result<()> {
    let w = world().await?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    w.save("area", "g1", None).await?;
    w.save("area", "g2", Some("area:g1")).await?;
    w.save("rule", "r", Some("area:g2")).await?;
    w.save("check", "a", Some("rule:r")).await?;
    w.save("check", "b", Some("rule:r")).await?;
    let nodes = ["rule:r", "area:g2", "area:g1"];
    // Fixture 1: A is green and B is grey.
    w.bind("check:a", &["a"]).await?;
    w.report(&[("a", "pass")]).await?;
    ensure!(w.tones(&["check:a", "check:b"]).await? == ["green", "grey"]);
    ensure!(
        w.tones(&nodes).await? == ["grey", "grey", "grey"],
        "fixture 1"
    );
    // Fixture 2: B is green.
    w.bind("check:b", &["b"]).await?;
    w.report(&[("a", "pass"), ("b", "pass")]).await?;
    ensure!(
        w.tones(&nodes).await? == ["green", "green", "green"],
        "fixture 2"
    );
    // Fixture 3: B is red.
    w.report(&[("a", "pass"), ("b", "fail")]).await?;
    ensure!(w.tones(&nodes).await? == ["red", "red", "red"], "fixture 3");
    // Fixture 4: A is grey and B is red.
    w.bind("check:a", &[]).await?;
    ensure!(w.tones(&["check:a", "check:b"]).await? == ["grey", "red"]);
    ensure!(w.tones(&nodes).await? == ["red", "red", "red"], "fixture 4");
    // Fixture 5: B is red, R2 has grey check D, and areas put grey before red.
    w.bind("check:a", &["a"]).await?;
    w.report(&[("a", "pass"), ("b", "fail")]).await?;
    w.save("rule", "r2", Some("area:g2")).await?;
    w.save("check", "d", Some("rule:r2")).await?;
    let tree = w.tree().await?;
    let mut types = tree["schema"]["types"].clone();
    types["area"]["status_order"] = json!(["grey", "red", "green"]);
    w.command(&w.owner, json!({"op":"set_tree_schema","revision":tree["current_revision"],"types":types,"reason":"Put grey first"})).await?;
    ensure!(
        w.tones(&["rule:r", "rule:r2", "area:g2", "area:g1"])
            .await?
            == ["red", "grey", "grey", "grey"],
        "fixture 5"
    );
    Ok(())
}

// type-status-rules-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn type_status_rules_ac3_empty_nodes_are_grey_and_uncounted_children_change_nothing()
-> Result<()> {
    let w = world().await?;
    let task = w.rules_schema(json!(["red", "grey", "green"])).await?;
    let task = task.as_str().context("task kind")?;
    w.save("area", "h", None).await?;
    w.save("rule", "q", Some("area:h")).await?;
    w.save("rule", "r", Some("area:h")).await?;
    w.save("check", "a", Some("rule:r")).await?;
    w.save("check", "c", Some("rule:r")).await?;
    w.save("memo", "m", Some("rule:r")).await?;
    w.bind("check:a", &["a"]).await?;
    w.bind("check:c", &["c"]).await?;
    w.report(&[("a", "pass"), ("c", "fail")]).await?;
    w.blocked_task(task, "check:a").await?;
    // Q has no checks: grey. R holds green A, red C and memo M: red.
    ensure!(w.tones(&["rule:q", "check:a", "rule:r"]).await? == ["grey", "green", "red"]);
    // A new area with only an empty rule stays grey.
    w.save("area", "e", None).await?;
    w.save("rule", "q2", Some("area:e")).await?;
    ensure!(w.tones(&["area:e"]).await? == ["grey"]);
    // Fixture 2: C is archived; then a new record passes every test of A.
    let c = w.object("check:c").await?;
    w.command(&w.owner, json!({"op":"archive_object","target":"check:c","revision":c["current_revision"],"reason":"Drop the check"})).await?;
    ensure!(
        w.tones(&["rule:r"]).await? == ["grey"],
        "right after the archive"
    );
    w.report(&[("a", "pass")]).await?;
    ensure!(
        w.tones(&["rule:r"]).await? == ["green"],
        "after the new report"
    );
    Ok(())
}

// type-status-rules-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn type_status_rules_ac5_a_schema_without_status_rules_uses_the_defaults() -> Result<()> {
    let w = world().await?;
    let tree = w.tree().await?;
    let types = &tree["schema"]["types"];
    // The schema of a new workspace names the rule of each type: the rules that the
    // roles gave before.
    ensure!(
        types.as_object().is_some_and(|t| t.values().all(|v| {
            let rule = match v["role"].as_str() {
                Some("ac") => "tests",
                Some("group" | "requirement") => "children",
                _ => "none",
            };
            v["status"] == rule
        })),
        "{types}"
    );
    let kind = |role: &str| -> Result<String> {
        types
            .as_object()
            .and_then(|t| t.iter().find(|(_, v)| v["role"] == role))
            .map(|(k, _)| k.clone())
            .with_context(|| format!("{role} type"))
    };
    let (requirement, criterion, task) = (kind("requirement")?, kind("ac")?, kind("task")?);
    w.save("consumer", "person", None).await?;
    w.save("domain", "d", None).await?;
    w.save("capability", "k", Some("domain:d")).await?;
    w.save_for(&requirement, "r", Some("capability:k"), Some("person"))
        .await?;
    let r = format!("{requirement}:r");
    w.save_for(&criterion, "a", Some(&r), Some("person"))
        .await?;
    w.save_for(&criterion, "b", Some(&r), Some("person"))
        .await?;
    let (a, b) = (format!("{criterion}:a"), format!("{criterion}:b"));
    // B has no test yet, so it is grey.
    w.bind(&a, &["a"]).await?;
    w.report(&[("a", "pass")]).await?;
    w.blocked_task(&task, &a).await?;
    let nodes = [
        a.as_str(),
        b.as_str(),
        r.as_str(),
        "capability:k",
        "domain:d",
    ];
    let tones = w.tones(&nodes).await?;
    ensure!(
        tones == ["green", "grey", "grey", "grey", "grey"],
        "{tones:?}"
    );
    // B turns green.
    w.bind(&b, &["b"]).await?;
    w.report(&[("a", "pass"), ("b", "pass")]).await?;
    let tones = w.tones(&nodes).await?;
    ensure!(
        tones == ["green", "green", "green", "green", "green"],
        "{tones:?}"
    );
    ensure!(
        w.tree().await?["current_revision"] == tree["current_revision"],
        "the reads changed the schema"
    );
    Ok(())
}

impl World {
    async fn mcp(&self, token: &str, tool: &str, mut arguments: Value) -> Result<Value> {
        arguments["workspace"] = json!(self.base.rsplit('/').next().unwrap());
        let reply: Value = self
            .http
            .post(format!("{}/mcp", self.root))
            .bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}}))
            .send()
            .await?
            .json()
            .await?;
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .with_context(|| format!("MCP reply: {reply}"))?;
        ensure!(reply["result"]["isError"] != true, "{tool}: {text}");
        Ok(serde_json::from_str(text)?)
    }
    /// Mark a criterion ready for work, as its owner.
    async fn ready(&self, criterion: &str) -> Result<()> {
        let p = self.object(criterion).await?["process"].clone();
        self.command(&self.owner, json!({"op":"assess","req":p["req"],"revision":p["revision"],"ac":p["criterion"],"assessment":{"ready":true,"reason":"Known scope","recipient":null,"decision":null}})).await?;
        Ok(())
    }
    async fn set_priority(
        &self,
        token: &str,
        target: &str,
        priority: &str,
    ) -> Result<(u16, Value)> {
        self.try_command(
            token,
            json!({"op":"set_priority","target":target,"priority":priority}),
        )
        .await
    }
    /// The addresses of the unstarted list that start with `prefix`, in list order.
    async fn unstarted(&self, prefix: &str) -> Result<Vec<String>> {
        let list = self
            .mcp(
                &self.bot,
                "joppa_objects",
                json!({"work_filter":"unstarted","limit":100}),
            )
            .await?;
        Ok(list["items"]
            .as_array()
            .context("items")?
            .iter()
            .filter_map(|i| i["address"].as_str())
            .filter(|a| a.starts_with(prefix))
            .map(String::from)
            .collect())
    }
}
