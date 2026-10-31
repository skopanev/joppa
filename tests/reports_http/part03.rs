// Part 3 of tests/reports_http.rs, included there, so the test names stay unchanged.

impl Planning<'_> {
    async fn write(&self, actor: &str, command: Value) -> Result<Value> {
        // An agent or a runner enrolls through the internal event, as an agent token does.
        if command["op"] == "enroll_service" {
            return store::enroll_service(
                &self.pool,
                &self.ws,
                actor,
                command["id"].as_str().unwrap_or_default(),
                command["role"].as_str().unwrap_or_default(),
            )
            .await;
        }
        store::execute_remote_input(
            &self.pool,
            &self.ws,
            actor,
            &uuid::Uuid::new_v4().to_string(),
            &command,
        )
        .await
    }
    async fn read(&self, address: &str) -> Result<Value> {
        store::read_objects(
            &self.pool,
            &self.ws,
            "owner",
            &store::ObjectScope {
                address: Some(address),
                ..Default::default()
            },
        )
        .await
    }
    /// Save a new title and contract of a Task and keep its other fields.
    async fn amend(
        &self,
        actor: &str,
        address: &str,
        title: &str,
        contract: &str,
    ) -> Result<Value> {
        let current = self.read(address).await?;
        let (kind, id) = address.split_once(':').unwrap();
        let mut data = current["document"]["data"].clone();
        data["title"] = json!(title);
        data["body"] = json!(contract);
        self.write(actor, json!({"op":"save_object","kind":kind,"id":id,"revision":current["current_revision"],"parent":current["document"]["parent"],"data":data,"links":current["document"]["links"],"reason":"Amend the contract"})).await
    }
    async fn revision(&self, address: &str) -> Result<u64> {
        Ok(self.read(address).await?["current_revision"]
            .as_u64()
            .unwrap_or(0))
    }
    async fn save(&self, key: &str, parent: Option<&str>, data: Value, links: Value) -> Result<()> {
        let (kind, id) = key.split_once(':').unwrap();
        let revision = match self.read(key).await {
            Ok(object) => object["current_revision"].as_u64().unwrap_or(0),
            Err(_) => 0,
        };
        self.write("owner", json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"links":links,"reason":"Set the contract"})).await?;
        Ok(())
    }
    /// Confirm `req` and save each assessment.
    async fn confirm(&self, req: &str, assessments: &[(&str, Value)]) -> Result<()> {
        let revision = self.read(req).await?["process"]["revision"].clone();
        self.write(
            "owner",
            json!({"op":"confirm","req":req,"revision":revision}),
        )
        .await?;
        for (ac, assessment) in assessments {
            self.write("owner", json!({"op":"assess","req":req,"revision":revision,"ac":ac,"assessment":assessment})).await?;
        }
        Ok(())
    }
    async fn ready(&self) -> Result<()> {
        let ready = json!({"ready":true,"reason":"Known scope"});
        self.confirm(
            "spec:one",
            &[("criterion:a", ready.clone()), ("criterion:b", ready)],
        )
        .await
    }
    /// Save prerequisite C in spec:two with `description` and keep it assessed ready.
    async fn prerequisite(&self, description: &str) -> Result<u64> {
        if self.revision("spec:two").await.unwrap_or(0) == 0 {
            self.save(
                "spec:two",
                None,
                json!({"title":"Prerequisite","body":"Deliver the prerequisite","owner":"owner"}),
                json!([]),
            )
            .await?;
        }
        self.save(
            "criterion:c",
            Some("spec:two"),
            json!({"title":"criterion:c","owner":"owner","body":description}),
            json!([]),
        )
        .await?;
        self.confirm(
            "spec:two",
            &[("criterion:c", json!({"ready":true,"reason":"Known scope"}))],
        )
        .await?;
        self.revision("criterion:c").await
    }
    /// A depends on C at `revision`.
    async fn depend(&self, revision: u64, blocking: bool) -> Result<()> {
        self.save("criterion:a", Some("spec:one"), json!({"title":"criterion:a","owner":"owner"}),
            json!([{"target":"criterion:c","relation":"depends_on","revision":revision,"blocking":blocking}])).await
    }
    /// T0: an existing Task under A.
    async fn t0(&self) -> Result<()> {
        self.save("work:t0", Some("criterion:a"), json!({"title":"T0","owner":"owner","body":"Implement this criterion","purpose":"implementation"}), json!([])).await
    }
    async fn claim(&self, actor: &str, ac: &str) -> Result<()> {
        let revision = self.revision(ac).await?;
        self.write(
            actor,
            json!({"op":"claim_planning","ac":ac,"revision":revision}),
        )
        .await?;
        Ok(())
    }
    /// Committed rows of the workspace: documents, revisions, links, process data,
    /// planning claims, receipts and the journal, each in a stable order.
    async fn saved(&self) -> Result<Value> {
        let client = self.pool.get().await?;
        let mut saved = json!({});
        // Table names are fixed literals; SQL cannot bind identifiers.
        for table in [
            "events",
            "objects",
            "object_revisions",
            "object_links",
            "state_entries",
            "state_revisions",
            "board_cache",
            "command_receipts",
        ] {
            let rows: String = client
                .query_one(
                    &format!("SELECT COALESCE(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)),'[]')::text FROM joppa.{table} t WHERE workspace=$1"),
                    &[&self.ws],
                )
                .await?
                .get(0);
            saved[table] = serde_json::from_str(&rows)?;
        }
        let position: i64 = client
            .query_one(
                "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1",
                &[&self.ws],
            )
            .await?
            .get(0);
        saved["position"] = json!(position);
        Ok(saved)
    }
    async fn saved_object(&self, address: &str) -> Result<Value> {
        let saved = self.saved().await?;
        Ok(saved["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["address"] == address)
            .cloned()
            .unwrap_or(Value::Null))
    }
    /// Fixture 1: A is ready and has no planning claim. L (`other`) holds the Task
    /// at revision 4 and blocks it. Returns the position of the block event.
    async fn blocked(&self, task: &str) -> Result<u64> {
        self.ready().await?;
        self.save(task, Some("criterion:a"), json!({"title":"T","owner":"owner","body":"Implement this criterion","purpose":"implementation"}), json!([])).await?;
        for (op, reason) in [
            ("claim_task", None),
            ("release_task", Some("Return the Task")),
            ("claim_task", None),
        ] {
            let mut command = json!({"op":op,"id":task,"revision":self.revision(task).await?});
            if let Some(reason) = reason {
                command["reason"] = json!(reason);
            }
            self.write("other", command).await?;
        }
        ensure!(self.revision(task).await? == 4);
        let reply = self.write("other", json!({"op":"block_task","id":task,"revision":4,"reason":"Verification failed","evidence":"test x failed on abc123"})).await?;
        reply["position"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("block reply has no position: {reply}"))
    }
}
