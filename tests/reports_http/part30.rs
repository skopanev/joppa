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
    /// The addresses of work_filter=unstarted over HTTP and MCP.
    async fn unstarted(&self) -> Result<Vec<String>> {
        let page = self
            .objects_both(
                "work_filter=unstarted&limit=100",
                json!({"work_filter":"unstarted","limit":100}),
            )
            .await?;
        Ok(page["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["address"].as_str().map(str::to_owned))
            .collect())
    }
    /// Bot reads joppa_tasks over HTTP and MCP; both give the same reply.
    async fn tasks_both(&self, view: &str) -> Result<Vec<Value>> {
        let http = self.get(format!("/tasks?view={view}&limit=100")).await?;
        let mcp = self
            .mcp_read("joppa_tasks", json!({"view":view,"limit":100}))
            .await?;
        ensure!(
            compact_of::compact_of(&http, &mcp) == mcp,
            "joppa_tasks {view}: HTTP {http}\nMCP {mcp}"
        );
        Ok(http["tasks"].as_array().cloned().unwrap_or_default())
    }
    async fn status(&self, ac: &str) -> Result<Value> {
        self.get(format!("/tests?ac={ac}")).await
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
    /// P saves the current document of `key` with `links` over HTTP.
    async fn link(&self, key: &str, links: Value) -> Result<Value> {
        let (kind, id) = key.split_once(':').unwrap();
        let object = self.read(key).await?;
        let document = &object["document"];
        self.owner_command(json!({"op":"save_object","kind":kind,"id":id,"revision":object["current_revision"],
            "parent":document["parent"],"data":document["data"],"links":links,"reason":"Order the work"}))
            .await
    }
    /// Readiness without the revision number of the specification.
    async fn readiness(&self, ac: &str) -> Result<Value> {
        let a = &self.read(ac).await?["process"]["assessment"];
        Ok(json!([
            a["state"],
            a["reason"],
            a["recipient"],
            a["decision"]
        ]))
    }
}

/// A waiting criterion reads work_ready=false and names `target` as its readiness blocker.
fn waits_for(object: &Value, target: &str) -> bool {
    object["work_ready"] == false
        && object["process"]["readiness_blocker"]["target"] == target
        && object["waits_for"]["address"] == target
        && object["process"]["readiness_blocker"]["reason"]
            .as_str()
            .is_some_and(|r| r.contains(target))
}

// dependencies-before-priority-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac1_a_criterion_waits_until_its_dependency_is_green_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1"]).await?;
    p.link("spec:b", ordering_link("spec:a")).await?;
    let b = p.object_both("spec:b").await?;
    ensure!(
        b["document"]["links"]
            == json!([{"target":"spec:a","relation":"depends_on","blocking":true}]),
        "the link stores no revision: {b}"
    );
    // Controls: A1 is ready and grey. The priority control is pending: set_priority is absent.
    let a1 = p.object_both("criterion:a1").await?;
    ensure!(
        a1["process"]["assessment"]["state"] == "ready" && a1["work_ready"] == true,
        "{a1}"
    );
    ensure!(p.status("criterion:a1").await?["status"] == "grey");
    let listed = p.unstarted().await?;
    ensure!(
        listed.contains(&"criterion:a1".to_owned()) && !listed.contains(&"criterion:b1".to_owned()),
        "{listed:?}"
    );
    let b1 = p.object_both("criterion:b1").await?;
    ensure!(
        b1["process"]["assessment"]["state"] == "ready" && waits_for(&b1, "spec:a"),
        "{b1}"
    );
    // A planning claim on B1 names A and adds no journal entry.
    p.refuse(
        "planning B1 before A is green",
        "criterion:b1",
        b1["current_revision"].as_u64().unwrap(),
        &joppa::dependencies::reason("spec:a"),
    )
    .await?;

    // A record passes every listed test of A1.
    p.write(
        "agent",
        json!({"op":"set_ac_tests","target":"criterion:a1","tests":[A],"reason":"Bind the test"}),
    )
    .await?;
    let report = outcomes(&[("opens", "pass"), ("refuses", "pass")]);
    p.record("c1c1c1c", report.clone()).await?;
    ensure!(p.status("criterion:a1").await?["status"] == "green");
    p.owner_command(
        json!({"op":"note","target":"cap:c","id":"unrelated","text":"An unrelated remark"}),
    )
    .await?;
    for repeat in [false, true] {
        if repeat {
            // The same record again is a new receipt of it.
            p.record("c1c1c1c", report.clone()).await?;
        }
        ensure!(
            p.unstarted().await?.contains(&"criterion:b1".to_owned()),
            "repeat={repeat}"
        );
        let b1 = p.object_both("criterion:b1").await?;
        ensure!(
            b1["work_ready"] == true
                && b1["process"]["readiness_blocker"].is_null()
                && b1.get("waits_for").is_none(),
            "repeat={repeat}: {b1}"
        );
    }

    // A Task pin to a changed Task contract still holds its dependent after a rebuild.
    let contract = |title: &str| json!({"title":title,"owner":"owner","body":"Implement the agreed mechanism","purpose":"implementation"});
    p.save(
        "work:source",
        Some("criterion:a1"),
        contract("Source"),
        json!([]),
    )
    .await?;
    let pin = p.revision("work:source").await?;
    p.save(
        "work:dependent",
        Some("criterion:a1"),
        contract("Dependent"),
        json!([{"target":"work:source","relation":"depends_on","revision":pin,"blocking":true}]),
    )
    .await?;
    p.amend(
        "owner",
        "work:source",
        "Changed source",
        "Implement the agreed mechanism",
    )
    .await?;
    let unclaimable = |tasks: Vec<Value>| !tasks.iter().any(|t| t["address"] == "work:dependent");
    ensure!(unclaimable(p.tasks_both("available").await?));
    ensure!(p.read("work:dependent").await?["review_required"] == true);
    for table in ["state_entries", "board_cache"] {
        p.pool
            .get()
            .await?
            .execute(
                &format!("DELETE FROM joppa.{table} WHERE workspace=$1"),
                &[&p.ws],
            )
            .await?;
    }
    store::migrate(&p.pool).await?;
    ensure!(unclaimable(p.tasks_both("available").await?));
    let dependent = p.read("work:dependent").await?;
    ensure!(
        dependent["review_required"] == true
            && dependent["document"]["links"][0]["revision"] == pin,
        "{dependent}"
    );
    ensure!(p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    store::check_journal(&p.pool).await?;
    Ok(())
}
