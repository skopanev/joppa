// Part 3b of tests/reports_http.rs, included there, so the test names stay unchanged.

impl Planning<'_> {
    /// P sends the claim through HTTP and then MCP. Each refusal returns `expected`
    /// and leaves the committed workspace rows unchanged.
    async fn refuse(&self, case: &str, ac: &str, revision: u64, expected: &str) -> Result<()> {
        let command = json!({"op":"claim_planning","ac":ac,"revision":revision});
        self.refuse_command(case, command, expected).await
    }
    /// P posts the request body to the HTTP command endpoint.
    async fn http(&self, body: Value) -> Result<(u16, Value)> {
        self.http_as(&self.w.token, body).await
    }
    /// The `token` member posts the request body to the HTTP command endpoint.
    async fn http_as(&self, token: &str, body: Value) -> Result<(u16, Value)> {
        let root = self.w.base.split("/api/").next().unwrap();
        let reply = self
            .w
            .http
            .post(format!("{root}/api/workspaces/{}/commands", self.ws))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;
        let code = reply.status().as_u16();
        Ok((code, reply.json().await.unwrap_or(Value::Null)))
    }
    /// P calls one MCP tool in this workspace.
    async fn mcp(&self, tool: &str, arguments: Value) -> Result<Value> {
        self.mcp_as(&self.w.token, tool, arguments).await
    }
    /// The `token` member calls one MCP tool in this workspace.
    async fn mcp_as(&self, token: &str, tool: &str, mut arguments: Value) -> Result<Value> {
        let root = self.w.base.split("/api/").next().unwrap();
        arguments["workspace"] = json!(self.ws);
        Ok(self.w.http.post(format!("{root}/mcp")).bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}}))
            .send().await?.json().await?)
    }
    /// P sends `command` through HTTP or MCP and gets the result or the error text.
    async fn send(&self, mcp: bool, command: &Value) -> Result<std::result::Result<Value, String>> {
        self.send_as(&self.w.token, mcp, command).await
    }
    /// The `token` member sends `command` through HTTP or MCP.
    async fn send_as(
        &self,
        token: &str,
        mcp: bool,
        command: &Value,
    ) -> Result<std::result::Result<Value, String>> {
        let request_id = uuid::Uuid::new_v4().to_string();
        if mcp {
            let response = self
                .mcp_as(
                    token,
                    "joppa_command",
                    json!({"request_id":request_id,"command":command}),
                )
                .await?;
            let text = response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            return Ok(if response["result"]["isError"] == true {
                Err(text)
            } else {
                Ok(serde_json::from_str(&text)?)
            });
        }
        let (code, reply) = self
            .http_as(token, json!({"request_id":request_id,"command":command}))
            .await?;
        Ok(if code == 200 {
            Ok(reply)
        } else {
            Err(format!("{code} {reply}"))
        })
    }
    /// P sends the command through HTTP and then MCP. Each refusal returns `expected`
    /// and leaves the committed workspace rows unchanged.
    async fn refuse_command(&self, case: &str, command: Value, expected: &str) -> Result<()> {
        self.refuse_command_as(&self.w.token, case, command, expected)
            .await
    }
    /// The `token` member sends the command through HTTP and then MCP, with the same checks.
    async fn refuse_command_as(
        &self,
        token: &str,
        case: &str,
        command: Value,
        expected: &str,
    ) -> Result<()> {
        let before = self.saved().await?;
        let (code, reply) = self
            .http_as(
                token,
                json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
            )
            .await?;
        ensure!(
            (400..500).contains(&code) && reply["error"] == expected,
            "{case} over HTTP: expected {expected:?}, got {code} {reply}"
        );
        ensure!(
            self.saved().await? == before,
            "{case}: the HTTP refusal changed saved state"
        );
        let response = self
            .mcp_as(
                token,
                "joppa_command",
                json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
            )
            .await?;
        ensure!(
            response["result"]["isError"] == true
                && response["result"]["content"][0]["text"] == expected,
            "{case} over MCP: expected {expected:?}, got {response}"
        );
        ensure!(
            self.saved().await? == before,
            "{case}: the MCP refusal changed saved state"
        );
        Ok(())
    }
}
