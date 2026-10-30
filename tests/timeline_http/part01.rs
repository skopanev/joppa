// Part 1 of tests/timeline_http.rs, included there, so the test names stay unchanged.

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Workspace W with the members bot (agent), pat and maria (persons), and runner (agent,
/// records test results).
struct World {
    _server: Server,
    _dir: std::path::PathBuf,
    pool: deadpool_postgres::Pool,
    ws: String,
    root: String,
    http: reqwest::Client,
    tokens: std::collections::BTreeMap<&'static str, String>,
}

const TYPES: &str = r#"{
    "domain":{"title":"Domains","role":"group","status":"children","root":true,"parents":[]},
    "capability":{"title":"Capabilities","role":"group","status":"children","parents":["domain"]},
    "requirement":{"title":"Requirements","role":"requirement","status":"children","parents":["capability"]},
    "ac":{"title":"Criteria","role":"ac","status":"tests","parents":["requirement"]},
    "signal":{"title":"Signals","role":"document","status":"none","root":true,"parents":[],"inbox":true}
}"#;

async fn world() -> Result<World> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    ensure!(
        url.rsplit('/')
            .next()
            .is_some_and(|db| db.starts_with("joppa_test")),
        "test database required"
    );
    let pool = store::pool(&url)?;
    static MIGRATED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    MIGRATED.get_or_try_init(|| store::migrate(&pool)).await?;
    let ws = format!("timeline-{}", uuid::Uuid::new_v4().simple());
    store::execute_input(
        &pool,
        &ws,
        "owner",
        "create",
        &json!({"op":"workspace","title":"Timeline"}),
    )
    .await?;
    for (id, role) in [
        ("bot", "agent"),
        ("pat", "human"),
        ("maria", "human"),
        ("runner", "agent"),
    ] {
        if role == "human" {
            store::execute_remote_input(
                &pool,
                &ws,
                "owner",
                &uuid::Uuid::new_v4().to_string(),
                &json!({"op":"member","id":id,"role":role}),
            )
            .await?;
        } else {
            store::enroll_service(&pool, &ws, "owner", id, role).await?;
        }
    }
    let dir =
        std::env::temp_dir().join(format!("joppa-timeline-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let tokens: std::collections::BTreeMap<&'static str, String> =
        ["owner", "bot", "pat", "maria", "runner"]
            .into_iter()
            .map(|actor| (actor, uuid::Uuid::new_v4().simple().to_string()))
            .collect();
    let file = dir.join("tokens.json");
    std::fs::write(
        &file,
        Value::Object(
            tokens
                .iter()
                .map(|(actor, token)| (token.clone(), json!(actor)))
                .collect(),
        )
        .to_string(),
    )?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("JOPPA_DATABASE_URL", &url)
            .env("JOPPA_TOKENS_FILE", &file)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .env("DEEPINFRA_API_KEY", "test-only-key")
            .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
            .envs(fake_spaces::start().env())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    for _ in 0..100 {
        if reqwest::get(format!("{root}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            let w = World {
                _server: server,
                _dir: dir,
                pool,
                ws,
                root,
                http: reqwest::Client::new(),
                tokens,
            };
            let schema = w.get("owner", "/tree").await?;
            let types: Value = serde_json::from_str(TYPES)?;
            w.command("owner", json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Configure the timeline tree"})).await?;
            return Ok(w);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}

impl World {
    fn token(&self, actor: &str) -> &str {
        &self.tokens[actor]
    }
    async fn get(&self, actor: &str, path: &str) -> Result<Value> {
        let reply = self
            .http
            .get(format!("{}/api/workspaces/{}{path}", self.root, self.ws))
            .bearer_auth(self.token(actor))
            .send()
            .await?;
        let code = reply.status();
        let body: Value = reply.json().await?;
        ensure!(code.is_success(), "GET {path}: {code} {body}");
        Ok(body)
    }
    async fn command(&self, actor: &str, command: Value) -> Result<Value> {
        let reply = self
            .http
            .post(format!("{}/api/workspaces/{}/commands", self.root, self.ws))
            .bearer_auth(self.token(actor))
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
            .send()
            .await?;
        let code = reply.status();
        let body: Value = reply.json().await?;
        ensure!(code.is_success(), "{actor}: {command}: {code} {body}");
        Ok(body)
    }
    async fn save(
        &self,
        actor: &str,
        address: &str,
        revision: u32,
        parent: Option<&str>,
        reason: &str,
    ) -> Result<Value> {
        let (kind, id) = address.split_once(':').unwrap();
        self.command(actor, json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,
            "data":{"title":format!("Title of {address}"),"owner":"pat","body":"Current source"},"reason":reason})).await
    }
    async fn record(&self, actor: &str, commit: &str) -> Result<Value> {
        let results = json!([{"test":"checks.cjs > one","status":"pass"}]);
        let token = self.token(actor);
        record_http::recorded(&self.http, &self.root, &self.ws, token, commit, results).await
    }
    async fn mcp(&self, token: &str, name: &str, arguments: Value) -> Result<Value> {
        let reply: Value = self
            .http
            .post(format!("{}/mcp", self.root))
            .bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(reply["result"]["isError"] != true, "{name}: {reply}");
        Ok(serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?)
    }
    /// The last journal position of W, read from the journal itself.
    async fn head(&self) -> Result<i64> {
        Ok(self
            .pool
            .get()
            .await?
            .query_one(
                "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1",
                &[&self.ws],
            )
            .await?
            .get(0))
    }
}
