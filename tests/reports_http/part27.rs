// Part 27 of tests/reports_http.rs, included there, so the test names stay unchanged.

impl Planning<'_> {
    async fn ac_tests(&self, ac: &str) -> Result<Value> {
        self.get(format!("/tests?ac={ac}")).await
    }
    async fn assessment(&self, ac: &str) -> Result<Value> {
        Ok(self.get(format!("/objects?address={ac}")).await?["process"]["assessment"].clone())
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
    /// Owner S saves R over HTTP with `parent`, `title` and `links`.
    async fn save_r(&self, parent: &str, title: &str, links: Value) -> Result<()> {
        let revision = self.revision("spec:r").await?;
        self.post_command(&self.w.owner, json!({"op":"save_object","kind":"spec","id":"r","revision":revision,"parent":parent,"data":{"title":title,"body":"Implement the rule","owner":"owner"},"links":links,"reason":"Change R"})).await
    }
    /// Agent bot saves the identical test list of A over HTTP.
    async fn resave_a(&self) -> Result<()> {
        self.post_command(&self.w.token, json!({"op":"set_ac_tests","target":"criterion:a","tests":[A,B],"reason":"Save the list again"})).await
    }
    /// Owner S saves inbox signal s under R over HTTP.
    async fn signal(&self) -> Result<()> {
        self.post_command(&self.w.owner, json!({"op":"save_object","kind":"signal","id":"s","revision":0,"parent":null,"data":{"title":"Provider change","owner":"owner","body":"New permission rules"},"links":[{"target":"spec:r","relation":"related"}],"reason":"Record the signal"})).await
    }
    /// A is grey with the test list of `before` and has no readiness assessment.
    async fn reset(&self, before: &Value) -> Result<()> {
        let s = self.ac_tests("criterion:a").await?;
        ensure!(
            s["status"] == "grey" && addresses(&s) == addresses(before),
            "A after the change: {s}"
        );
        let readiness = self.assessment("criterion:a").await?;
        ensure!(
            readiness["state"] == "unassessed",
            "A readiness: {readiness}"
        );
        Ok(())
    }
    /// A is still green with the test list of `before` and the readiness `assessment`.
    async fn kept(&self, before: &Value, assessment: &Value) -> Result<()> {
        let s = self.ac_tests("criterion:a").await?;
        ensure!(
            s["status"] == "green" && addresses(&s) == addresses(before),
            "A after the change: {s}"
        );
        ensure!(
            self.assessment("criterion:a").await? == *assessment,
            "the readiness of A changed"
        );
        Ok(())
    }
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f1_a_new_title_of_r_makes_a_grey_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    p.save_r("area:g", "R renamed", json!([])).await?;
    p.reset(&before).await?;
    let mcp = p
        .mcp_read("joppa_tests", json!({"ac":"criterion:a"}))
        .await?;
    ensure!(mcp["status"] == "grey", "MCP joppa_tests: {mcp}");
    Ok(())
}

// ready-is-the-only-gate-ac2
// R is requirement:key at revision 2 (its text, then its criterion), and A is its green ready
// criterion ac:key-rights. Agent bot saves a new text of R. Revision 1 of R keeps the old text.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_save_of_r_makes_a_new_revision_greys_a_and_resets_its_readiness() -> Result<()> {
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
    let tests = || async {
        let mcp = mcp_read(
            &w,
            &w.token,
            "joppa_tests",
            json!({"workspace":workspace,"ac":"ac:key-rights"}),
        )
        .await?;
        let http = w.status().await?;
        ensure!(
            compact_of::compact_of(&http, &mcp) == mcp,
            "joppa_tests: HTTP {http}\nMCP {mcp}"
        );
        anyhow::Ok(http)
    };
    ready_for_planning(&w).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    // Controls: R is at revision 2, and A is green and ready for work.
    let r = read(
        "address=requirement:key",
        json!({"address":"requirement:key"}),
    )
    .await?;
    ensure!(r["current_revision"] == 2, "R before the save: {r}");
    ensure!(tests().await?["status"] == "green", "A must start green");
    let a = read("address=ac:key-rights", json!({"address":"ac:key-rights"})).await?;
    ensure!(
        a["process"]["assessment"]["state"] == "ready",
        "A before the save: {a}"
    );
    let mut data = r["document"]["data"].clone();
    data["body"] = json!("Changed scope");
    let (code, reply) = post(&w, &w.token, "/commands", json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":
        {"op":"save_object","kind":"requirement","id":"key","revision":2,"parent":r["document"]["parent"],
         "data":data,"links":[],"reason":"Change R"}})).await?;
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
    let status = tests().await?;
    ensure!(status["status"] == "grey", "A after the save: {status}");
    let a = read("address=ac:key-rights", json!({"address":"ac:key-rights"})).await?;
    ensure!(
        a["process"]["assessment"]["state"] == "unassessed"
            && a["process"]["readiness_blocker"]["state"] == "unassessed",
        "A after the save: {a}"
    );
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
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    p.save_r(
        "area:g",
        "R",
        json!([{"target":"area:h","relation":"related"}]),
    )
    .await?;
    p.reset(&before).await
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f3_a_move_of_r_makes_a_grey() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    p.save_r("area:h", "R", json!([])).await?;
    p.reset(&before).await
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f4_a_comment_on_r_keeps_a_green() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let (before, assessment) = (
        p.ac_tests("criterion:a").await?,
        p.assessment("criterion:a").await?,
    );
    p.post_command(
        &w.owner,
        json!({"op":"note","target":"spec:r","id":"context","text":"Keep this context"}),
    )
    .await?;
    p.kept(&before, &assessment).await
}
