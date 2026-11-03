// Part 1 of tests/grey_restart.rs, included there, so the test names stay unchanged.

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
const TABLES: [&str; 6] = [
    "objects",
    "object_revisions",
    "object_links",
    "state_entries",
    "state_revisions",
    "search_documents",
];

const A: &str = "tests/auth/login.test.ts > login > opens";

const B: &str = "tests/auth/login.test.ts > login > refuses";

const OWNER: &str = "owner";

const AGENT: &str = "agent";

struct Fixture {
    pool: deadpool_postgres::Pool,
    ws: String,
    http: reqwest::Client,
    root: String,
    agent: String,
    owner: String,
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
    async fn count(&self, statement: &str) -> Result<i64> {
        Ok(self
            .pool
            .get()
            .await?
            .query_one(statement, &[&self.ws])
            .await?
            .get(0))
    }
    /// The stored test status of one criterion, from the test nodes under it.
    async fn tests(&self, ac: &str) -> Result<Value> {
        store::test_status(&self.pool, &self.ws, AGENT, ac).await
    }
    /// Bind each test to a criterion with one test node.
    async fn bind(&self, criterion: &str, tests: &[&str]) -> Result<()> {
        for test in tests {
            self.write(AGENT, json!({"op":"save_object","kind":"test","revision":0,"parent":criterion,"data":{"title":test,"owner":OWNER},"reason":"Bind the tests"})).await?;
        }
        Ok(())
    }
    /// Assert the status of A and D at one step and return both replies.
    async fn expect(&self, step: &str, a: &str, d: &str) -> Result<(Value, Value)> {
        let (ra, rd) = (
            self.tests("criterion:a").await?,
            self.tests("criterion:d").await?,
        );
        ensure!(ra["status"] == a, "{step}: A must be {a}: {ra}");
        ensure!(rd["status"] == d, "{step}: D must be {d}: {rd}");
        Ok((ra, rd))
    }
    async fn post(&self, path: &str, token: &str, body: Value) -> Result<Value> {
        let response = self
            .http
            .post(format!("{}/api/workspaces/{}/{path}", self.root, self.ws))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;
        let code = response.status();
        let reply: Value = response.json().await?;
        ensure!(code.is_success(), "POST {path}: {code} {reply}");
        Ok(reply)
    }
}
