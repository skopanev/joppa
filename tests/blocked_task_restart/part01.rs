// Part 1 of tests/blocked_task_restart.rs, included there, so the test names stay unchanged.

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn start(url: &str, tokens: &std::path::Path, port: u16) -> Result<Server> {
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("DEEPINFRA_API_KEY", "test-only-key")
            .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
            .envs(fake_spaces::start().env())
            .env("JOPPA_DATABASE_URL", url)
            .env("JOPPA_TOKENS_FILE", tokens)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    for _ in 0..100 {
        if reqwest::get(format!("http://127.0.0.1:{port}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(server);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("test server did not start")
}

/// Derived tables that server startup must rebuild from the journal.
const TABLES: [&str; 8] = [
    "objects",
    "object_revisions",
    "object_links",
    "ac_owners",
    "state_entries",
    "state_revisions",
    "task_queue",
    "search_documents",
];

const BLOCKED: &str = "Task is blocked for planning";

const PLANNER: &str = "planner";

const L: &str = "worker-l";

const Z: &str = "worker-z";

const OWNER: &str = "owner";

/// Each fixture Task under its own ready criterion.
const FIXTURES: [(&str, &str, &str); 4] = [
    ("F1", "work:t1", "criterion:f1"),
    ("F2", "work:t2", "criterion:f2"),
    ("F3", "work:d", "criterion:f3"),
    ("F4", "work:t4", "criterion:f4"),
];

struct Fixture {
    pool: deadpool_postgres::Pool,
    ws: String,
}

impl Fixture {
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
        .map_err(|e| anyhow::anyhow!("{actor}: {command}: {e:#}"))
    }
    async fn read(&self, address: &str) -> Result<Value> {
        store::read_objects(
            &self.pool,
            &self.ws,
            OWNER,
            &store::ObjectScope {
                address: Some(address),
                ..Default::default()
            },
        )
        .await
    }
    /// `actor` sends `command` for `id` at its current revision.
    async fn task(&self, actor: &str, id: &str, mut command: Value) -> Result<Value> {
        command["id"] = json!(id);
        command["revision"] = self.read(id).await?["current_revision"].clone();
        self.write(actor, command).await
    }
    async fn block(&self, actor: &str, id: &str) -> Result<Value> {
        let reply = self.task(actor, id, json!({"op":"block_task","reason":"Verification failed","evidence":"test x failed on abc123"})).await?;
        Ok(
            json!({"actor":actor,"reason":"Verification failed","evidence":"test x failed on abc123","position":reply["position"]}),
        )
    }
    /// Remove the workspace projections and its board cache; keep the journal.
    async fn clear_projections(&self) -> Result<()> {
        {
            let conn = self.pool.get().await?;
            for table in TABLES {
                let statement = format!("DELETE FROM joppa.{table} WHERE workspace=$1");
                conn.execute(&statement, &[&self.ws]).await?;
            }
            conn.execute(
                "DELETE FROM joppa.board_cache WHERE workspace=$1",
                &[&self.ws],
            )
            .await?;
        }
        let cache = "SELECT count(*) FROM joppa.board_cache WHERE workspace=$1";
        let queue = "SELECT count(*) FROM joppa.task_queue WHERE workspace=$1";
        let (cached, queued) = (self.count(cache).await?, self.count(queue).await?);
        ensure!(
            cached == 0 && queued == 0,
            "projections remain before startup: board_cache={cached} task_queue={queued}"
        );
        Ok(())
    }
    async fn count(&self, statement: &str) -> Result<i64> {
        Ok(self
            .pool
            .get()
            .await?
            .query_one(statement, &[&self.ws])
            .await?
            .get(0))
    }
}

/// The normalized reads of one transport, keyed by fixture name. Only the compared
/// fields are kept, so no read time can differ between reads.
type Snapshot = BTreeMap<&'static str, Value>;

/// The list of a reply: MCP gives `items`, HTTP gives `tasks` or `revisions`.
fn listed<'a>(reply: &'a Value, http_key: &str) -> &'a Value {
    if reply["items"].is_array() {
        &reply["items"]
    } else {
        &reply[http_key]
    }
}

/// One read of the token member: the HTTP `path`, or the MCP `tool` with `arguments`.
async fn fetch(
    http: &reqwest::Client,
    root: &str,
    ws: &str,
    token: &str,
    mcp: bool,
    (path, tool, mut arguments): (String, &str, Value),
) -> Result<Value> {
    if !mcp {
        let response = http
            .get(format!("{root}/api/workspaces/{ws}/{path}"))
            .bearer_auth(token)
            .send()
            .await?;
        let code = response.status();
        let body: Value = response.json().await?;
        ensure!(code.is_success(), "HTTP {path}: {code} {body}");
        return Ok(body);
    }
    arguments["workspace"] = json!(ws);
    let response: Value = http.post(format!("{root}/mcp")).bearer_auth(token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}}))
        .send().await?.json().await?;
    ensure!(
        response["result"]["isError"] != true,
        "MCP {tool}: {response}"
    );
    Ok(serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("null"),
    )?)
}
