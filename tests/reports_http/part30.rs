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
    /// The ready criteria over HTTP and MCP: the criteria that wait for nothing.
    async fn ready_work(&self) -> Result<Vec<String>> {
        let page = self
            .objects_both(
                "kind=criterion&ready=true&limit=100",
                json!({"kind":"criterion","ready":true,"limit":100}),
            )
            .await?;
        Ok(page["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["address"].as_str().map(str::to_owned))
            .collect())
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

/// A waiting criterion reads work_ready=false and names `target` with the reason.
fn waits_for(object: &Value, target: &str) -> bool {
    object["work_ready"] == false
        && object["waits_for"]["address"] == target
        && object["waits_for"]["reason"]
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
        depends_on_targets(&b) == ["spec:a"]
            && b["document"]["relations"]["depends_on"]["edges"][0]["revision"].is_null(),
        "the link stores no revision: {b}"
    );
    // Controls: A1 is ready and grey.
    let a1 = p.object_both("criterion:a1").await?;
    ensure!(a1["work_ready"] == true, "{a1}");
    ensure!(p.status("criterion:a1").await?["status"] == "grey");
    let listed = p.ready_work().await?;
    ensure!(
        listed.contains(&"criterion:a1".to_owned()) && !listed.contains(&"criterion:b1".to_owned()),
        "{listed:?}"
    );
    let b1 = p.object_both("criterion:b1").await?;
    ensure!(waits_for(&b1, "spec:a"), "{b1}");

    // A record passes every listed test of A1.
    p.bind(&w.token, "criterion:a1", &[A]).await?;
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
            p.ready_work().await?.contains(&"criterion:b1".to_owned()),
            "repeat={repeat}"
        );
        let b1 = p.object_both("criterion:b1").await?;
        ensure!(
            b1["work_ready"] == true && b1.get("waits_for").is_none(),
            "repeat={repeat}: {b1}"
        );
    }

    ensure!(p.ready_work().await?.contains(&"criterion:b1".to_owned()));
    store::check_journal(&p.pool).await?;
    Ok(())
}
