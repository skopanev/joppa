// Part 30 of tests/reports_http.rs, included there, so the test names stay unchanged.

impl Planning<'_> {
    /// Bot reads joppa_objects over HTTP and MCP; both give the same reply.
    async fn objects_both(&self, query: &str, arguments: Value) -> Result<Value> {
        let http = self.get(format!("/objects?{query}")).await?;
        let mcp = self.mcp_read("joppa_objects", arguments).await?;
        ensure!(
            compact_of::compact_of(&http, &mcp) == mcp,
            "joppa_objects {query}: HTTP {http}\nMCP {mcp}"
        );
        Ok(http)
    }
    async fn object_both(&self, address: &str) -> Result<Value> {
        self.objects_both(&format!("address={address}"), json!({"address":address}))
            .await
    }
    async fn status(&self, ac: &str) -> Result<Value> {
        store::test_status(&self.pool, &self.ws, "agent", ac).await
    }
    /// Member P (the owner) sends one command over HTTP.
    async fn owner_command(&self, command: Value) -> Result<Value> {
        let root = self.w.base.split("/api/").next().unwrap();
        let reply = self
            .w
            .http
            .post(format!("{root}/api/workspaces/{}/commands", self.ws))
            .bearer_auth(&self.w.owner)
            .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}))
            .send()
            .await?;
        let code = reply.status().as_u16();
        let body: Value = reply.json().await.unwrap_or(Value::Null);
        ensure!(code == 200, "{command}: {code} {body}");
        Ok(body)
    }
    /// P saves the current document of `key` with `relations` over HTTP. The relations merge
    /// by key, and a null key removes its edges.
    async fn link(&self, key: &str, relations: Value) -> Result<Value> {
        let (kind, id) = key.split_once(':').unwrap();
        let object = self.read(key).await?;
        let document = &object["document"];
        self.owner_command(json!({"op":"save_object","kind":kind,"id":id,"revision":object["current_revision"],
            "parent":document["parent"],"data":document["data"],"relations":relations,"reason":"Order the work"}))
            .await
    }
}

/// The targets of the stored depends_on edges of a read.
fn depends_on_targets(object: &Value) -> Vec<String> {
    object["document"]["relations"]["depends_on"]["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["to"].as_str().map(str::to_owned))
        .collect()
}
