// Part 27 of tests/reports_http.rs, included there, so the test names stay unchanged.

impl Planning<'_> {
    async fn ac_tests(&self, ac: &str) -> Result<Value> {
        store::test_status(&self.pool, &self.ws, "agent", ac).await
    }
    /// `token` posts `command` to the HTTP command endpoint with a new request ID.
    async fn post_command(&self, token: &str, command: Value) -> Result<()> {
        let root = self.w.base.split("/api/").next().unwrap();
        let reply = self
            .w
            .http
            .post(format!("{root}/api/workspaces/{}/commands", self.ws))
            .bearer_auth(token)
            .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}))
            .send()
            .await?;
        let code = reply.status();
        ensure!(code.is_success(), "{code} {}", reply.text().await?);
        Ok(())
    }
    /// Owner S saves R over HTTP with `parent`, `title` and `relations`. Only an object is
    /// sent as relations: a save with no edges omits them.
    async fn save_r(&self, parent: &str, title: &str, relations: Value) -> Result<()> {
        let revision = self.revision("spec:r").await?;
        let mut command = json!({"op":"save_object","kind":"spec","id":"r","revision":revision,"parent":parent,"data":{"title":title,"body":"Implement the rule","owner":"owner"},"reason":"Change R"});
        if relations.is_object() {
            command["relations"] = relations;
        }
        self.post_command(&self.w.owner, command).await
    }
    /// Owner S saves a new text of criterion A over HTTP, which resets only A.
    async fn change_a(&self) -> Result<()> {
        let revision = self.revision("criterion:a").await?;
        self.post_command(&self.w.owner, json!({"op":"save_object","kind":"criterion","id":"a","revision":revision,"parent":"spec:r","data":{"title":"criterion:a","owner":"owner","body":"A changed criterion"},"reason":"Change A"})).await
    }
    /// Owner S saves inbox signal s under R over HTTP.
    async fn signal(&self) -> Result<()> {
        self.post_command(&self.w.owner, json!({"op":"save_object","kind":"signal","id":"s","revision":0,"parent":null,"data":{"title":"Provider change","owner":"owner","body":"New permission rules"},"relations":{"related":{"edges":[{"to":"spec:r"}]}},"reason":"Record the signal"})).await
    }
    /// A is grey with the test list of `before`.
    async fn reset(&self, before: &Value) -> Result<()> {
        let s = self.ac_tests("criterion:a").await?;
        ensure!(
            s["status"] == "grey" && addresses(&s) == addresses(before),
            "A after the change: {s}"
        );
        Ok(())
    }
    /// A is still green with the test list of `before`.
    async fn kept(&self, before: &Value) -> Result<()> {
        let s = self.ac_tests("criterion:a").await?;
        ensure!(
            s["status"] == "green" && addresses(&s) == addresses(before),
            "A after the change: {s}"
        );
        Ok(())
    }
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f1_a_new_title_of_r_makes_a_grey_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w).await?;
    let before = p.ac_tests("criterion:a").await?;
    p.save_r("area:g", "R renamed", json!({})).await?;
    p.reset(&before).await?;
    let mcp = p
        .mcp_read("joppa_objects", json!({"address":"criterion:a"}))
        .await?;
    ensure!(mcp["status"]["tone"] == "grey", "MCP joppa_objects: {mcp}");
    Ok(())
}

// ready-is-the-only-gate-ac2
// R is requirement:key at revision 2 (its text, then its criterion), and A is its green
// criterion ac:key-rights. Agent bot saves a new text of R. Revision 1 of R keeps the old text.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_save_of_r_makes_a_new_revision_and_greys_a() -> Result<()> {
    let w = world().await?;
    let workspace = w.base.rsplit('/').next().unwrap().to_owned();
    // Bot reads joppa_objects over HTTP and MCP; both give the same reply.
    let read = |query: &'static str, mut arguments: Value| {
        let w = &w;
        arguments["workspace"] = json!(workspace);
        async move {
            let http: Value = w
                .http
                .get(format!("{}/objects?{query}", w.base))
                .bearer_auth(&w.token)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let mcp = mcp_read(w, &w.token, "joppa_objects", arguments).await?;
            ensure!(
                compact_of::compact_of(&http, &mcp) == mcp,
                "joppa_objects {query}: HTTP {http}\nMCP {mcp}"
            );
            anyhow::Ok(http)
        }
    };
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    // Controls: R is at revision 2, and A is green.
    let r = read(
        "address=requirement:key",
        json!({"address":"requirement:key"}),
    )
    .await?;
    ensure!(r["current_revision"] == 2, "R before the save: {r}");
    ensure!(w.status().await?["status"] == "green", "A must start green");
    let mut data = r["document"]["data"].clone();
    data["body"] = json!("Changed scope");
    let (code, reply) = post(&w, &w.token, "/commands", json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":
        {"op":"save_object","kind":"requirement","id":"key","revision":2,"parent":r["document"]["parent"],
         "data":data,"reason":"Change R"}})).await?;
    ensure!(code == 200, "bot saves R: {code} {reply}");
    let r = read(
        "address=requirement:key",
        json!({"address":"requirement:key"}),
    )
    .await?;
    ensure!(
        r["current_revision"] == 3
            && r["document"]["actor"] == "agent"
            && r["document"]["data"]["body"] == "Changed scope",
        "R after the save: {r}"
    );
    let status = w.status().await?;
    ensure!(status["status"] == "grey", "A after the save: {status}");
    let old = read(
        "address=requirement:key&revision=1",
        json!({"address":"requirement:key","revision":1}),
    )
    .await?;
    ensure!(
        old["historical"] == true
            && old["document"]["revision"] == 1
            && old["document"]["data"]["body"] == "Scope",
        "R at revision 1: {old}"
    );
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f2_a_new_link_of_r_makes_a_grey() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w).await?;
    let before = p.ac_tests("criterion:a").await?;
    p.save_r(
        "area:g",
        "R",
        json!({"related":{"edges":[{"to":"area:h"}]}}),
    )
    .await?;
    p.reset(&before).await
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f3_a_move_of_r_makes_a_grey() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w).await?;
    let before = p.ac_tests("criterion:a").await?;
    p.save_r("area:h", "R", json!({})).await?;
    p.reset(&before).await
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f4_a_comment_on_r_keeps_a_green() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w).await?;
    let before = p.ac_tests("criterion:a").await?;
    p.post_command(
        &w.owner,
        json!({"op":"note","target":"spec:r","id":"context","text":"Keep this context"}),
    )
    .await?;
    p.kept(&before).await
}
