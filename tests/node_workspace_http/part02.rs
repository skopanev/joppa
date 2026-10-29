// Part 2 of tests/node_workspace_http.rs, included there, so the test names stay unchanged.

impl World {
    /// One MCP tool call. A refusal gives Err with the exact error text.
    async fn mcp(&self, token: &str, tool: &str, args: Value) -> Result<Result<Value, String>> {
        let reply: Value = self
            .http
            .post(format!("{}/mcp", self.root))
            .bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}}))
            .send()
            .await?
            .json()
            .await?;
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .with_context(|| format!("{reply}"))?;
        Ok(if reply["result"]["isError"] == true {
            Err(text.to_owned())
        } else {
            Ok(serde_json::from_str(text)?)
        })
    }
    async fn command(
        &self,
        token: &str,
        ws: &str,
        command: Value,
    ) -> Result<Result<Value, String>> {
        let id = uuid::Uuid::new_v4().to_string();
        self.mcp(
            token,
            "joppa_command",
            json!({"workspace":ws,"request_id":id,"command":command}),
        )
        .await
    }
    async fn ok(&self, token: &str, ws: &str, command: Value) -> Result<Value> {
        self.command(token, ws, command.clone())
            .await?
            .map_err(|e| anyhow::anyhow!("{command}: {e}"))
    }
    async fn refused(&self, token: &str, ws: &str, command: Value, error: &str) -> Result<()> {
        match self.command(token, ws, command.clone()).await? {
            Err(e) if e.contains(error) => Ok(()),
            other => anyhow::bail!("{command}: expected {error:?}, got {other:?}"),
        }
    }
    async fn object(&self, token: &str, ws: &str, address: &str) -> Result<Value> {
        self.mcp(
            token,
            "joppa_objects",
            json!({"workspace":ws,"address":address}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{address}: {e}"))
    }
    async fn record_in(&self, ws: &str, commit: &str, status: &str) -> Result<()> {
        self.mcp(
            &self.bot,
            "joppa_record_results",
            json!({"workspace":ws,"commit":commit,"results":[{"test":TEST,"status":status}]}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("record: {e}"))?;
        Ok(())
    }
    async fn record(&self, commit: &str, status: &str) -> Result<()> {
        self.mcp(
            &self.bot,
            "joppa_record_results",
            json!({"workspace":self.c,"commit":commit,"results":[{"test":TEST,"status":status}]}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("record: {e}"))?;
        Ok(())
    }
    fn save(kind: &str, id: &str, revision: u64, parent: Option<&str>, data: Value) -> Value {
        json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Fixture"})
    }
    /// P: requirement R with criterion A, ready for planning. C: requirement API with
    /// criterion X that lists TEST.
    async fn fixture(&self) -> Result<()> {
        let (owner, p, c) = (&self.owner, &self.p, &self.c);
        self.ok(
            owner,
            p,
            Self::save(
                "requirement",
                "r",
                0,
                None,
                json!({"title":"R","owner":"owner","body":"The product rule"}),
            ),
        )
        .await?;
        self.ok(
            owner,
            p,
            Self::save(
                "ac",
                "a",
                0,
                Some("requirement:r"),
                json!({"title":"A","owner":"owner"}),
            ),
        )
        .await?;
        let a = self.object(owner, p, "ac:a").await?;
        self.ok(owner, p, json!({"op":"assess","req":a["document"]["parent"],"revision":a["process"]["assessment"]["revision"],"ac":a["address"],
            "assessment":{"ready":true,"reason":"The scope of A is known"}})).await?;
        self.ok(
            owner,
            c,
            Self::save(
                "requirement",
                "api",
                0,
                None,
                json!({"title":"API","owner":"owner","body":"The API contract"}),
            ),
        )
        .await?;
        self.ok(
            owner,
            c,
            Self::save(
                "ac",
                "x",
                0,
                Some("requirement:api"),
                json!({"title":"X","owner":"owner"}),
            ),
        )
        .await?;
        self.ok(
            owner,
            c,
            json!({"op":"set_ac_tests","target":"ac:x","tests":[TEST],"reason":"Bind"}),
        )
        .await?;
        Ok(())
    }
    fn link(&self, revision: Option<u64>, blocking: bool) -> Value {
        let mut link = json!({"workspace":self.c,"target":"requirement:api","relation":if blocking {"depends_on"} else {"related"},"blocking":blocking});
        if let Some(r) = revision {
            link["revision"] = json!(r);
        }
        link
    }
    /// The current revision of an object.
    async fn revision(&self, ws: &str, address: &str) -> Result<u64> {
        Ok(
            self.object(&self.owner, ws, address).await?["current_revision"]
                .as_u64()
                .unwrap_or_default(),
        )
    }
    /// Replace the remote links of one object, with the links revision that it reads first.
    async fn set_links(
        &self,
        token: &str,
        ws: &str,
        target: &str,
        links: Value,
    ) -> Result<Result<Value, String>> {
        // A read without remote links gives no revision: send 0.
        let current = self.object(&self.owner, ws, target).await?["remote_links_revision"]
            .as_u64()
            .unwrap_or(0);
        self.command(token, ws, json!({"op":"set_remote_links","target":target,"revision":current,"links":links,"reason":"Link to another workspace"})).await
    }
    /// A depends on the current revision of API with a blocking link, and A is ready again.
    async fn block_a_on_api(&self) -> Result<()> {
        let pin = self.revision(&self.c, "requirement:api").await?;
        self.set_links(
            &self.bot,
            &self.p,
            "ac:a",
            json!([self.link(Some(pin), true)]),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        let a = self.object(&self.owner, &self.p, "ac:a").await?;
        self.ok(&self.owner, &self.p, json!({"op":"assess","req":a["document"]["parent"],"revision":a["process"]["assessment"]["revision"],"ac":a["address"],
            "assessment":{"ready":true,"reason":"The scope of A is known"}})).await?;
        Ok(())
    }
    fn waiting(&self) -> String {
        format!("waits for @{}/requirement:api, which is not green", self.c)
    }
    async fn claim(&self) -> Result<Result<Value, String>> {
        let a = self.object(&self.bot, &self.p, "ac:a").await?;
        self.command(
            &self.bot,
            &self.p,
            json!({"op":"claim_planning","ac":"ac:a","revision":a["current_revision"]}),
        )
        .await
    }
}
