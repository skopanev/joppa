// Part 7 of tests/reports_http.rs, included there, so the test names stay unchanged.

/// G holds R with criterion A and child group G2 with criterion A2. Sibling group H
/// holds criterion D. Each criterion lists A and B, one record passes both,
/// and each criterion is assessed ready.
async fn branches(w: &World) -> Result<Planning<'_>> {
    let p = Planning {
        w,
        pool: store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?,
        ws: format!("http-branches-{}", uuid::Uuid::new_v4().simple()),
    };
    store::execute_input(
        &p.pool,
        &p.ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Branch reset"}),
    )
    .await?;
    p.write(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(&p.pool, &p.ws, "owner", None).await?;
    p.write("owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure nested groups","types":{
        "area":{"title":"Area","role":"group","status":"children","root":true,"parents":["area"]},
        "spec":{"title":"Specification","role":"requirement","status":"children","parents":["area"]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]}
    }})).await?;
    for (key, parent) in [
        ("area:g", None),
        ("area:g2", Some("area:g")),
        ("area:h", None),
    ] {
        p.save(
            key,
            parent,
            json!({"title":key,"body":"Group scope","owner":"owner"}),
            json!([]),
        )
        .await?;
    }
    for (spec, area, criterion) in BRANCH_CRITERIA {
        p.save(
            spec,
            Some(area),
            json!({"title":spec,"body":"Implement the rule","owner":"owner"}),
            json!([]),
        )
        .await?;
        p.save(
            criterion,
            Some(spec),
            json!({"title":criterion,"owner":"owner"}),
            json!([]),
        )
        .await?;
        p.write(
            "agent",
            json!({"op":"set_ac_tests","target":criterion,"tests":[A,B],"reason":"Bind the tests"}),
        )
        .await?;
    }
    p.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    for (spec, _, criterion) in BRANCH_CRITERIA {
        p.confirm(
            spec,
            &[(criterion, json!({"ready":true,"reason":"Known scope"}))],
        )
        .await?;
    }
    Ok(p)
}

const BRANCH_CRITERIA: [(&str, &str, &str); 3] = [
    ("spec:r", "area:g", "criterion:a"),
    ("spec:r2", "area:g2", "criterion:a2"),
    ("spec:rh", "area:h", "criterion:d"),
];

impl Planning<'_> {
    /// Record `results` for `commit` in this workspace with the member token.
    async fn record(&self, commit: &str, results: Value) -> Result<()> {
        let root = self.w.base.split("/api/").next().unwrap();
        record_http::recorded(&self.w.http, root, &self.ws, &self.w.token, commit, results).await?;
        Ok(())
    }
    /// GET one path of this workspace over HTTP.
    async fn get(&self, path: String) -> Result<Value> {
        let root = self.w.base.split("/api/").next().unwrap();
        Ok(self
            .w
            .http
            .get(format!("{root}/api/workspaces/{}{path}", self.ws))
            .bearer_auth(&self.w.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    /// The owner saves `key` over HTTP with its current parent and no links.
    async fn owner_save(&self, key: &str, parent: Option<&str>, data: Value) -> Result<()> {
        let (kind, id) = key.split_once(':').unwrap();
        let root = self.w.base.split("/api/").next().unwrap();
        let command = json!({"op":"save_object","kind":kind,"id":id,"revision":self.revision(key).await?,"parent":parent,"data":data,"links":[],"reason":"Change the group"});
        self.w
            .http
            .post(format!("{root}/api/workspaces/{}/commands", self.ws))
            .bearer_auth(&self.w.owner)
            .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
    /// The parsed text of one MCP tool result.
    async fn mcp_read(&self, tool: &str, arguments: Value) -> Result<Value> {
        let reply = self.mcp(tool, arguments).await?;
        Ok(serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?)
    }
    async fn boundaries(&self) -> Result<Vec<(String, String)>> {
        Ok(self.pool.get().await?.query("SELECT key,value::text FROM joppa.state_entries WHERE workspace=$1 AND section='criterion_boundaries' ORDER BY key", &[&self.ws]).await?
            .iter().map(|r| (r.get(0), r.get(1))).collect())
    }
}

fn addresses(status: &Value) -> Vec<Value> {
    status["tests"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| t["address"].clone())
        .collect()
}
