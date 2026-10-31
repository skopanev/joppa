// Part 2 of tests/status_rules_http.rs, included there, so the test names stay unchanged.

impl World {
    async fn try_command(&self, token: &str, command: Value) -> Result<(u16, Value)> {
        let reply = self
            .http
            .post(format!("{}/commands", self.base))
            .bearer_auth(token)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
            .send()
            .await?;
        Ok((reply.status().as_u16(), reply.json().await?))
    }
    async fn command(&self, token: &str, command: Value) -> Result<Value> {
        let (code, reply) = self.try_command(token, command.clone()).await?;
        ensure!(code == 200, "{command}: {code} {reply}");
        Ok(reply)
    }
    async fn tree(&self) -> Result<Value> {
        Ok(self
            .http
            .get(format!("{}/tree", self.base))
            .bearer_auth(&self.owner)
            .send()
            .await?
            .json()
            .await?)
    }
    async fn object(&self, address: &str) -> Result<Value> {
        Ok(self
            .http
            .get(format!("{}/objects?address={address}", self.base))
            .bearer_auth(&self.alex)
            .send()
            .await?
            .json()
            .await?)
    }
    async fn mcp_object(&self, address: &str) -> Result<Value> {
        let reply: Value = self
            .http
            .post(format!("{}/mcp", self.root))
            .bearer_auth(&self.alex)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":self.base.rsplit('/').next().unwrap(),"address":address}}}))
            .send()
            .await?
            .json()
            .await?;
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .with_context(|| format!("MCP reply: {reply}"))?;
        Ok(serde_json::from_str(text)?)
    }
    /// The tone of each address over HTTP; MCP must give the same tone.
    async fn tones(&self, addresses: &[&str]) -> Result<Vec<String>> {
        let mut tones = vec![];
        for address in addresses {
            let http = self.object(address).await?;
            let mcp = self.mcp_object(address).await?;
            let tone = http["signal"]["tone"].as_str().unwrap_or("").to_owned();
            ensure!(
                mcp["status"]["tone"] == tone.as_str(),
                "{address}: HTTP {tone}, MCP {}",
                mcp["status"]["tone"]
            );
            tones.push(tone);
        }
        Ok(tones)
    }
    /// A record from bot with each test as pass or fail, on a new commit.
    async fn report(&self, cases: &[(&str, &str)]) -> Result<()> {
        self.order.set(self.order.get() + 1);
        let order = self.order.get();
        let results = record_http::cases(SUITE, cases);
        let ws = self.base.rsplit('/').next().unwrap();
        let commit = format!("c{order:06}");
        record_http::recorded(&self.http, &self.root, ws, &self.bot, &commit, results).await?;
        Ok(())
    }
    async fn save(&self, kind: &str, id: &str, parent: Option<&str>) -> Result<()> {
        self.save_for(kind, id, parent, None).await
    }
    /// Save a node; a type that requires consumers names consumer `person`. A configured
    /// tree reads the list `consumers`, the built-in tree the single `consumer`.
    async fn save_for(
        &self,
        kind: &str,
        id: &str,
        parent: Option<&str>,
        consumer: Option<&str>,
    ) -> Result<()> {
        let mut data = json!({"title":id,"owner":"owner","body":"The contract","body":"Given: a node.\nWhen: alex reads it.\nThen: it has a tone.\nEvidence: the tone."});
        if let Some(consumer) = consumer {
            data["consumers"] = json!([consumer]);
            data["consumer"] = json!(consumer);
        }
        self.command(&self.owner, json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"reason":"Build the tree"})).await?;
        Ok(())
    }
    async fn bind(&self, address: &str, tests: &[&str]) -> Result<()> {
        let tests: Vec<String> = tests.iter().map(|t| format!("{SUITE} > {t}")).collect();
        self.command(
            &self.owner,
            json!({"op":"set_ac_tests","target":address,"tests":tests,"reason":"Bind the test"}),
        )
        .await?;
        Ok(())
    }
    /// Save the schema of ac1: area and rule climb from children, check takes its tests,
    /// memo has no status field. Task may sit under a check.
    async fn rules_schema(&self, area_order: Value) -> Result<Value> {
        let tree = self.tree().await?;
        let mut types = tree["schema"]["types"].clone();
        types["area"] = json!({"title":"Areas","role":"group","root":true,"parents":["area"],"status":"children","status_order":area_order});
        types["rule"] =
            json!({"title":"Rules","role":"requirement","parents":["area"],"status":"children"});
        types["check"] = json!({"title":"Checks","role":"ac","parents":["rule"],"status":"tests"});
        types["memo"] =
            json!({"title":"Memos","role":"document","status":"none","parents":["rule"]});
        let task = types
            .as_object()
            .and_then(|t| t.iter().find(|(_, v)| v["role"] == "task"))
            .map(|(k, _)| k.clone())
            .context("task type")?;
        types[&task]["parents"]
            .as_array_mut()
            .context("task parents")?
            .push(json!("check"));
        self.command(&self.owner, json!({"op":"set_tree_schema","revision":tree["current_revision"],"types":types,"reason":"Set the status rules"})).await?;
        Ok(json!(task))
    }
    /// Task T under a check, claimed and blocked by bot.
    async fn blocked_task(&self, task_kind: &str, check: &str) -> Result<()> {
        let id = format!("t-{}", check.replace(':', "-"));
        let parent = json!(check);
        self.command(&self.owner, json!({"op":"save_object","kind":task_kind,"id":id,"revision":0,"parent":parent,"data":{"title":"Implement","owner":"owner","body":"Implement and check the contract","purpose":"implementation"},"reason":"Create work"})).await?;
        let criterion = self.object(check).await?;
        let p = &criterion["process"];
        self.command(&self.owner, json!({"op":"assess","req":p["req"],"revision":p["revision"],"ac":p["criterion"],"assessment":{"ready":true,"reason":"Known scope","recipient":null,"decision":null}})).await?;
        let address = format!("{task_kind}:{id}");
        let revision = self.object(&address).await?["current_revision"].clone();
        self.command(
            &self.bot,
            json!({"op":"claim_task","id":address,"revision":revision}),
        )
        .await?;
        let revision = self.object(&address).await?["current_revision"].clone();
        self.command(&self.bot, json!({"op":"block_task","id":address,"revision":revision,"reason":"The test fails","evidence":"test a failed"})).await?;
        ensure!(
            self.object(&address).await?["process"]["blocked"].is_object(),
            "Task is not blocked"
        );
        Ok(())
    }
}

// type-status-rules-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn type_status_rules_ac1_the_owner_saves_a_status_rule_for_each_type() -> Result<()> {
    let w = world().await?;
    let before = w.tree().await?["current_revision"]
        .as_u64()
        .context("revision")?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    let tree = w.tree().await?;
    let revision = tree["current_revision"].as_u64().context("revision")?;
    ensure!(revision == before + 1, "{before} -> {revision}");
    let types = &tree["schema"]["types"];
    // The MCP tool joppa_tree shows the same revision and the same saved rules, without
    // the false type flags.
    let mcp = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    let lean = joppa::mcp_lean::lean("joppa_tree", &json!({}), tree.clone());
    ensure!(
        mcp["schema"]["revision"] == tree["current_revision"]
            && mcp["schema"]["types"] == lean["schema"]["types"],
        "{mcp}"
    );
    ensure!(
        types["check"]["status"] == "tests"
            && types["rule"]["status"] == "children"
            && types["area"]["status"] == "children"
            && types["area"]["status_order"] == json!(["red", "grey", "green"])
            && types["memo"]["status"] == "none",
        "{types}"
    );
    for (field, value, error) in [
        (
            "status",
            json!("parent"),
            "a status rule is tests, children, links or none",
        ),
        (
            "status_order",
            json!(["red", "blue"]),
            "a status order allows only red, grey and green",
        ),
    ] {
        let mut changed = types.clone();
        changed["area"][field] = value;
        let (code, reply) = w
            .try_command(&w.owner, json!({"op":"set_tree_schema","revision":revision,"types":changed,"reason":"Try a wrong rule"}))
            .await?;
        ensure!(
            code != 200 && reply["error"].as_str().is_some_and(|e| e.contains(error)),
            "{field}: {code} {reply}"
        );
        ensure!(
            w.tree().await?["current_revision"] == revision,
            "{field} changed the schema"
        );
        ensure!(
            w.mcp(&w.owner, "joppa_tree", json!({})).await?["schema"]["revision"] == revision,
            "{field} changed the schema over MCP"
        );
    }
    Ok(())
}
