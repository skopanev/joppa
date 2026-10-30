// Part 1 of tests/node_workspace_http.rs, included there, so the test names stay unchanged.

const TEST: &str = "tests/api.rs > api";

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
    root: String,
    p: String,
    c: String,
    owner: String,
    bot: String,
    alex: String,
    pool: deadpool_postgres::Pool,
    http: reqwest::Client,
}

const TYPES: &str = r#"{
    "component":{"title":"Components","role":"group","status":"children","root":true,"parents":[]},
    "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
    "ac":{"title":"Criteria","role":"ac","status":"tests","parents":["requirement"]},
    "task":{"title":"Tasks","role":"task","status":"none","parents":["ac"]}
}"#;

async fn workspace(pool: &deadpool_postgres::Pool, prefix: &str, alex: bool) -> Result<String> {
    let ws = format!("{prefix}-{}", uuid::Uuid::new_v4().simple());
    let write = async |actor: &str, command: Value| {
        let id = uuid::Uuid::new_v4().to_string();
        store::execute_remote_input(pool, &ws, actor, &id, &command).await
    };
    let op: joppa::commands::Command =
        serde_json::from_value(json!({"op":"workspace","title":prefix}))?;
    store::execute(pool, &ws, "owner", "setup", &op).await?;
    if alex {
        write("owner", json!({"op":"member","id":"alex","role":"human"})).await?;
    }
    store::enroll_service(pool, &ws, "owner", "bot", "agent").await?;
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    write("owner", json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":serde_json::from_str::<Value>(TYPES)?})).await?;
    Ok(ws)
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
    let p = workspace(&pool, "product", true).await?;
    let c = workspace(&pool, "component", false).await?;
    let dir = std::env::temp_dir().join(format!("joppa-nw-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let token = || uuid::Uuid::new_v4().simple().to_string();
    let (owner, bot, alex) = (token(), token(), token());
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({ owner.clone(): "owner", bot.clone(): "bot", alex.clone(): "alex" }).to_string(),
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
                root,
                p,
                c,
                owner,
                bot,
                alex,
                pool,
                http: reqwest::Client::new(),
            };
            w.fixture().await?;
            return Ok(w);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}
