// Part 1 of tests/reports_http.rs, included there, so the test names stay unchanged.

/// The data of one criterion under requirement:key.
fn criterion(title: &str) -> Value {
    json!({"title":title,"owner":"owner","consumer":"customer","before_launch":false,"decision_required":false,"depends_on":[]})
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct World {
    _server: Server,
    _dir: std::path::PathBuf,
    base: String,
    token: String,
    owner: String,
    /// The token of the member `other`, who is Q in the planning tests.
    other: String,
    /// The token of alex, a person who pushes in the hook tests.
    alex: String,
    http: reqwest::Client,
    pool: deadpool_postgres::Pool,
}

const A: &str = "tests/auth/login.test.ts > login > opens";

const B: &str = "tests/auth/login.test.ts > login > refuses";

async fn world() -> Result<World> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    ensure!(
        url.rsplit('/')
            .next()
            .is_some_and(|db| db.starts_with("joppa_test")),
        "test database required"
    );
    let pool = store::pool(&url)?;
    // Parallel migrations of one database deadlock on their DDL: migrate once per test binary.
    static MIGRATED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    MIGRATED.get_or_try_init(|| store::migrate(&pool)).await?;
    let ws = format!("http-reports-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Reports over HTTP"}),
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
        json!({"op":"save_object","kind":"consumer","id":"customer","revision":0,"parent":null,"data":{"title":"Customer","owner":"owner"},"reason":"Setup"}),
        json!({"op":"save_object","kind":"domain","id":"security","revision":0,"parent":null,"data":{"title":"Security","owner":"owner"},"reason":"Setup"}),
        json!({"op":"save_object","kind":"capability","id":"restrict","revision":0,"parent":"domain:security","data":{"title":"Restrict access","owner":"owner"},"reason":"Setup"}),
        json!({"op":"save_object","kind":"requirement","id":"key","revision":0,"parent":"capability:restrict","data":{"title":"Key rights","body":"Scope","owner":"owner"},"reason":"Agreed"}),
        json!({"op":"save_object","kind":"ac","id":"key-rights","revision":0,"parent":"requirement:key","data":criterion("Only read and verify before launch"),"reason":"Agreed"}),
        json!({"op":"save_object","kind":"task","id":"t1","revision":0,"parent":"ac:key-rights","data":{"title":"Build key rights","owner":"owner","projects":[],"body":"Implement and provide evidence"},"reason":"Plan the work"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&pool, &ws, "owner", &format!("setup-{i}"), &op).await?;
    }
    let dir = std::env::temp_dir().join(format!("joppa-reports-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let owner = uuid::Uuid::new_v4().simple().to_string();
    let other = uuid::Uuid::new_v4().simple().to_string();
    let alex = uuid::Uuid::new_v4().simple().to_string();
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({ token.clone(): "agent", owner.clone(): "owner", other.clone(): "other", alex.clone(): "alex" })
            .to_string(),
    )?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("DEEPINFRA_API_KEY", "test-only-key")
            .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
            .envs(fake_spaces::start().env())
            .env("JOPPA_DATABASE_URL", &url)
            .env("JOPPA_TOKENS_FILE", &tokens)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    for _ in 0..100 {
        if reqwest::get(format!("{root}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            let world = World {
                _server: server,
                _dir: dir,
                base: format!("{root}/api/workspaces/{ws}"),
                token,
                owner,
                other,
                alex,
                http: reqwest::Client::new(),
                pool,
            };
            world.list(&[A, B]).await?;
            return Ok(world);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}

impl World {
    /// Make the test nodes of ac:key-rights name exactly `tests`.
    async fn list(&self, tests: &[&str]) -> Result<()> {
        bind_at(&self.http, &self.base, &self.token, "ac:key-rights", tests).await
    }
    /// The server address and the workspace name of this world.
    fn root_ws(&self) -> (&str, &str) {
        self.base.split_once("/api/workspaces/").unwrap()
    }
    /// Record `results` for `commit` with the member token.
    async fn record(&self, commit: &str, results: Value) -> Result<Value> {
        let (root, ws) = self.root_ws();
        record_http::recorded(&self.http, root, ws, &self.token, commit, results).await
    }
    /// skopanev's role in these tests: change the criterion text, which makes a new revision.
    async fn revise_text(&self, _expected: u32, text: &str) -> Result<()> {
        let current: Value = self
            .http
            .get(format!("{}/objects?address=ac:key-rights", self.base))
            .bearer_auth(&self.owner)
            .send()
            .await?
            .json()
            .await?;
        let mut data = current["document"]["data"].clone();
        data["title"] = json!(text);
        let command = json!({"op":"save_object","kind":"ac","id":"key-rights","revision":current["current_revision"],"parent":"requirement:key","data":data,"links":current["document"]["links"],"reason":"Sharper wording"});
        let reply = self
            .http
            .post(format!("{}/commands", self.base))
            .bearer_auth(&self.owner)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
            .send()
            .await?;
        ensure!(
            reply.status().is_success(),
            "revise: {}",
            reply.text().await?
        );
        Ok(())
    }
    /// The test status of ac:key-rights.
    async fn status(&self) -> Result<Value> {
        store::test_status(&self.pool, self.root_ws().1, "agent", "ac:key-rights").await
    }
}

/// Outcomes of tests/auth/login.test.ts > login, as joppa_record_results takes them.
fn outcomes(cases: &[(&str, &str)]) -> Value {
    record_http::cases("tests/auth/login.test.ts > login", cases)
}

async fn ready_for_planning(w: &World) -> Result<Value> {
    for command in [
        json!({"op":"assess","target":"ac:key-rights","ready":true,"reason":"Known work scope"}),
    ] {
        let (code, reply) = post(
            w,
            &w.owner,
            "/commands",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
        )
        .await?;
        ensure!(code == 200, "{code} {reply}");
    }
    planning_criterion(w).await
}

async fn planning_criterion(w: &World) -> Result<Value> {
    Ok(w.http
        .get(format!("{}/objects?address=ac:key-rights", w.base))
        .bearer_auth(&w.token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
