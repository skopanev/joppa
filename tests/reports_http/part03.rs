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
}
