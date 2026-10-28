// Part 1 of tests/spec_http.rs, included there, so the test names stay unchanged.

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct World {
    _server: Server,
    root: String,
    ws: String,
    owner: String,
    alex: String,
    maria: String,
    bot: String,
    skopanev: String,
    /// A signed-in account that is not a member of the workspace.
    eve: String,
    pool: deadpool_postgres::Pool,
    http: reqwest::Client,
}

/// Create a workspace with the shared spec setup, then run `extra` as (actor, command).
async fn seed(pool: &deadpool_postgres::Pool, ws: &str, extra: Vec<(&str, Value)>) -> Result<()> {
    let setup = [
        json!({"op":"workspace","title":"Spec over HTTP"}),
        json!({"op":"member","id":"alex","role":"human"}),
        json!({"op":"member","id":"maria","role":"human"}),
        json!({"op":"enroll_service","id":"bot","role":"agent"}),
        json!({"op":"consumer","id":"customer","title":"Customer","owner":"owner"}),
        json!({"op":"domain","id":"security","title":"Security","authority":"owner"}),
        json!({"op":"capability","id":"restrict","title":"Restrict access","domain":"security"}),
        json!({"op":"capability","id":"empty","title":"Nothing below","domain":"security"}),
        json!({"op":"revise","req":"key","expected_revision":0,"capability":"restrict","title":"Key rights","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"key-rights","text":"Only read and verify before launch"}]}),
    ]
    .into_iter()
    .map(|op| ("owner", op))
    .chain(extra);
    for (i, (actor, op)) in setup.enumerate() {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(pool, ws, actor, &format!("setup-{i}"), &op).await?;
    }
    Ok(())
}

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
    let ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    seed(&pool, &ws, vec![]).await?;
    let dir = std::env::temp_dir().join(format!("joppa-spec-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let owner = uuid::Uuid::new_v4().simple().to_string();
    let alex = uuid::Uuid::new_v4().simple().to_string();
    let maria = uuid::Uuid::new_v4().simple().to_string();
    let bot = uuid::Uuid::new_v4().simple().to_string();
    let skopanev = uuid::Uuid::new_v4().simple().to_string();
    let eve = uuid::Uuid::new_v4().simple().to_string();
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({ owner.clone(): "owner", alex.clone(): "alex", maria.clone(): "maria", bot.clone(): "bot", skopanev.clone(): "skopanev", eve.clone(): "eve" }).to_string(),
    )?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("JOPPA_DATABASE_URL", &url)
            .env("JOPPA_TOKENS_FILE", &tokens)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .env("DEEPINFRA_API_KEY", "test-only-key")
            .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    for _ in 0..100 {
        if reqwest::get(format!("{root}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(World {
                _server: server,
                root,
                ws,
                owner,
                alex,
                maria,
                bot,
                skopanev,
                eve,
                pool,
                http: reqwest::Client::new(),
            });
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}

impl World {
    async fn command(&self, command: Value) -> Result<Value> {
        self.command_as(&self.owner, command).await
    }
    async fn command_as(&self, token: &str, command: Value) -> Result<Value> {
        let reply = self
            .http
            .post(format!("{}/api/workspaces/{}/commands", self.root, self.ws))
            .bearer_auth(token)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
            .send()
            .await?;
        ensure!(
            reply.status().is_success(),
            "{command}: {}",
            reply.text().await?
        );
        Ok(reply.json().await?)
    }
    async fn get(&self, query: &str) -> Result<Value> {
        self.get_as(&self.owner, query).await
    }
    async fn get_as(&self, token: &str, query: &str) -> Result<Value> {
        let reply = self
            .http
            .get(format!(
                "{}/api/workspaces/{}/objects?{query}",
                self.root, self.ws
            ))
            .bearer_auth(token)
            .send()
            .await?;
        ensure!(
            reply.status().is_success(),
            "{query}: {}",
            reply.text().await?
        );
        Ok(reply.json().await?)
    }
    async fn note(
        &self,
        id: &str,
        revision: u32,
        title: &str,
        body: &str,
        links: Value,
    ) -> Result<()> {
        self.command(json!({"op":"save_object","kind":"note","id":id,"revision":revision,"parent":null,"data":{"title":title,"owner":"owner","body":body},"links":links,"reason":"Save"})).await?;
        Ok(())
    }
    async fn search(&self, query: &str) -> Result<Vec<String>> {
        let reply: Value = self
            .http
            .post(format!("{}/mcp", self.root))
            .bearer_auth(&self.owner)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_search","arguments":{"verbose":true,"workspace":self.ws,"query":query}}}))
            .send()
            .await?
            .json()
            .await?;
        let found: Value = serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        let rows = found["results"]
            .as_array()
            .or(found["items"].as_array())
            .cloned()
            .unwrap_or_default();
        Ok(rows
            .iter()
            .filter_map(|r| Some(format!("{}:{}", r["kind"].as_str()?, r["id"].as_str()?)))
            .collect())
    }
    /// The first search result, only when it is an exact ID or address match.
    async fn first_exact(&self, query: &str) -> Result<Option<String>> {
        let reply: Value = self
            .http
            .post(format!("{}/mcp", self.root))
            .bearer_auth(&self.owner)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_search","arguments":{"verbose":true,"workspace":self.ws,"query":query}}}))
            .send()
            .await?
            .json()
            .await?;
        let found: Value = serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        let first = &found["items"][0];
        Ok((first["exact_id"] == true).then(|| {
            format!(
                "{}:{}",
                first["kind"].as_str().unwrap_or(""),
                first["id"].as_str().unwrap_or("")
            )
        }))
    }
}
