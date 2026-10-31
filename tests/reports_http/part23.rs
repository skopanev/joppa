// Part 23 of tests/reports_http.rs, included there, so the test names stay unchanged.

impl Planning<'_> {
    /// The `token` member reads `path` over HTTP and through the MCP `tool`.
    async fn reads(
        &self,
        token: &str,
        path: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<[(&'static str, Value); 2]> {
        let root = self.w.base.split("/api/").next().unwrap();
        let http: Value = self
            .w
            .http
            .get(format!("{root}/api/workspaces/{}/{path}", self.ws))
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mcp = self.mcp_as(token, tool, arguments).await?;
        ensure!(mcp["result"]["isError"] != true, "MCP {tool}: {mcp}");
        let mcp = serde_json::from_str(
            mcp["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        Ok([("HTTP", http), ("MCP", mcp)])
    }
    /// One Task view of the `token` member over HTTP and MCP. MCP omits a false
    /// `claimable` or `review_required`; the checks read a missing flag as false.
    async fn view(&self, token: &str, view: &str) -> Result<[(&'static str, Value); 2]> {
        let [http, (mcp, mut page)] = self
            .reads(
                token,
                &format!("tasks?view={view}&limit=100"),
                "joppa_tasks",
                json!({"view":view,"limit":100}),
            )
            .await?;
        for item in page["items"].as_array_mut().into_iter().flatten() {
            for flag in ["claimable", "review_required"] {
                if item.get(flag).is_none() {
                    item[flag] = json!(false);
                }
            }
        }
        Ok([http, (mcp, page)])
    }
    /// The view=all entry of `address` for the `token` member over HTTP and MCP.
    async fn entry(&self, token: &str, address: &str) -> Result<[(&'static str, Value); 2]> {
        let [(http, h), (mcp, m)] = self.view(token, "all").await?;
        let find = |page: Value| {
            queued(&page, address)
                .ok_or_else(|| anyhow::anyhow!("{address} is missing in view=all: {page}"))
        };
        Ok([(http, find(h)?), (mcp, find(m)?)])
    }
    /// One object read of the `token` member over HTTP and MCP.
    async fn object(&self, token: &str, address: &str) -> Result<[(&'static str, Value); 2]> {
        self.reads(
            token,
            &format!("objects?address={address}"),
            "joppa_objects",
            json!({"address":address}),
        )
        .await
    }
    /// `actor` sends `command` through the store and gets its journal position.
    async fn position(&self, actor: &str, command: Value) -> Result<u64> {
        let reply = self.write(actor, command).await?;
        reply["position"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("reply has no position: {reply}"))
    }
    /// The saved revisions of `address`, in revision order.
    async fn history(&self, address: &str) -> Result<Vec<Value>> {
        let mut rows: Vec<Value> = self.saved().await?["object_revisions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["address"] == address)
            .cloned()
            .collect();
        rows.sort_by_key(|r| r["revision"].as_u64());
        Ok(rows)
    }
    /// The workspace owner saves A with `description` and confirms it ready again.
    async fn revise_a(&self, description: &str) -> Result<u64> {
        self.save(
            "criterion:a",
            Some("spec:one"),
            json!({"title":"criterion:a","owner":"owner","body":description}),
            json!([]),
        )
        .await?;
        self.ready().await?;
        self.revision("criterion:a").await
    }
    /// The view=all values of T for P: each transport must satisfy `check`.
    async fn expect_t(&self, point: &str, check: impl Fn(&Value) -> bool) -> Result<()> {
        for (transport, t) in self.entry(&self.w.token, "work:t").await? {
            ensure!(check(&t), "{point} {transport}: {t}");
        }
        Ok(())
    }
    /// AC7 fixture: T with `links` is planned at A revision 4. A is at revision 5,
    /// so T needs review. P claims A and saves T with a new contract.
    async fn replanned(&self, links: Value) -> Result<()> {
        let mut revision = self.revision("criterion:a").await?;
        while revision < 4 {
            revision = self.revise_a(&format!("Wording {revision}")).await?;
        }
        ensure!(revision == 4, "A revision {revision}");
        self.save("work:t", Some("criterion:a"), json!({"title":"T","owner":"owner","body":"Implement this criterion","purpose":"implementation"}), links.clone()).await?;
        self.expect_t("at A revision 4", |t| t["review_required"] == false)
            .await?;
        ensure!(self.revise_a("Fifth wording").await? == 5);
        self.expect_t("at A revision 5", |t| t["review_required"] == true)
            .await?;
        self.claim("agent", "criterion:a").await?;
        self.expect_t("after the claim", |t| t["review_required"] == true)
            .await?;
        self.plan_task(
            "work:t",
            "Implement the criterion with a new contract",
            links,
        )
        .await?;
        self.expect_t("after the holder save", |t| t["review_required"] == true)
            .await
    }
    /// AC3 fixture: W (`worker`) holds T0 under ready A, and P holds A.
    async fn held_t0(&self) -> Result<()> {
        self.ready().await?;
        self.t0().await?;
        self.write(
            "owner",
            json!({"op":"enroll_service","id":"worker","role":"agent"}),
        )
        .await?;
        let revision = self.revision("work:t0").await?;
        self.write(
            "worker",
            json!({"op":"claim_task","id":"work:t0","revision":revision}),
        )
        .await?;
        self.claim("agent", "criterion:a").await
    }
    /// P saves T0 of W with a new contract.
    async fn update_t0(&self) -> Result<()> {
        let revision = self.revision("work:t0").await?;
        self.write("agent", json!({"op":"save_object","kind":"work","id":"t0","revision":revision,"parent":"criterion:a","data":{"title":"T0","owner":"owner","body":"Implement the updated plan","purpose":"implementation"},"reason":"Update the older work"})).await?;
        Ok(())
    }
    /// The planning claim of `ac` on both transports.
    async fn planning_claims(&self, ac: &str) -> Result<Vec<(&'static str, Value)>> {
        Ok(self
            .object(&self.w.token, ac)
            .await?
            .into_iter()
            .map(|(transport, a)| (transport, a["process"]["planning_claim"].clone()))
            .collect())
    }
    /// P still holds A, and each new Task is unavailable for publication.
    async fn unpublished(&self, tasks: &[&str]) -> Result<()> {
        for (transport, claim) in self.planning_claims("criterion:a").await? {
            ensure!(claim["actor"] == "agent", "{transport}: {claim}");
        }
        for task in tasks {
            for (transport, t) in self.entry(&self.w.token, task).await? {
                ensure!(
                    t["claimable"] == false && t["waiting_reason"] == AWAITING,
                    "{transport} {task}: {t}"
                );
            }
        }
        Ok(())
    }
}

/// The entry of `address` on one Task view page.
fn queued(page: &Value, address: &str) -> Option<Value> {
    task_list(page)
        .as_array()
        .and_then(|tasks| tasks.iter().find(|t| t["address"] == address))
        .cloned()
}

fn release_command(reason: &str) -> Value {
    json!({"op":"release_planning","ac":"criterion:a","reason":reason})
}

/// Delete the board cache and rebuild the projections from the journal.
async fn rebuild(p: &Planning<'_>) -> Result<()> {
    p.pool
        .get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&p.ws])
        .await?;
    store::migrate(&p.pool).await
}

// planning-claim-ac7
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac7_a_replanned_task_follows_the_current_criterion_revision_after_finish() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.replanned(json!([])).await?;
        p.finish(mcp).await?;
        let published = |t: &Value| {
            t["review_required"] == false && t["claimable"] == true && t["waiting_reason"].is_null()
        };
        p.expect_t("after finish", published).await?;
        store::check_journal(&p.pool).await?;
        rebuild(&p).await?;
        p.expect_t("after replay", published).await?;
        ensure!(p.revise_a("Sixth wording").await? == 6);
        p.expect_t("at A revision 6", |t| t["review_required"] == true)
            .await?;
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}
