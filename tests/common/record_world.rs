//! One workspace on its own real server for the record tests. Agent bot, member alex and member
//! another-member belong to it. Outsider is signed in, but is not a member. Criterion ac:a lists
//! the tests of the fixture. The setup writes through the store. The tests write through MCP.
#![allow(dead_code)]
#[path = "fake_spaces.rs"]
pub mod fake_spaces;
#[path = "serve.rs"]
pub mod serve;
use anyhow::{Context, Result, ensure};
use joppa::{commands::Command as Operation, store};
use serde_json::{Value, json};

pub const A: &str = "ac:a"; // The criterion of each fixture.

pub struct World {
    server: Option<serve::Server>,
    url: String,
    port: u16,
    dir: std::path::PathBuf,
    env: Vec<(String, String)>,
    pub root: String,
    pub ws: String,
    pub owner: String,
    pub bot: String,
    pub alex: String,
    pub outsider: String,
    pub pool: deadpool_postgres::Pool,
    pub http: reqwest::Client,
}

/// Start a server for a new workspace in which criterion A lists `tests`.
pub async fn world(tests: &[&str]) -> Result<World> {
    build(tests, None).await
}

/// `world` with the variables `env` and JOPPA_PUBLIC_URL of its own root for the server.
pub async fn world_env(tests: &[&str], env: &[(&str, String)]) -> Result<World> {
    build(tests, Some(env)).await
}

/// The variables `env` and JOPPA_PUBLIC_URL `root`, for a server process.
fn vars(root: &str, env: &[(&str, String)]) -> Vec<(String, String)> {
    let mut vars = vec![("JOPPA_PUBLIC_URL".to_owned(), root.to_owned())];
    vars.extend(env.iter().map(|(k, v)| ((*k).to_owned(), v.clone())));
    vars
}

async fn build(tests: &[&str], env: Option<&[(&str, String)]>) -> Result<World> {
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
    let ws = format!("record-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Record channel"}),
        json!({"op":"member","id":"alex","role":"human"}),
        json!({"op":"member","id":"another-member","role":"human"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&pool, &ws, "owner", &format!("setup-{i}"), &op).await?;
    }
    store::enroll_service(&pool, &ws, "owner", "bot", "agent").await?;
    let tree = store::tree_schema(&pool, &ws, "owner", None).await?;
    let write = async |actor: &str, command: Value| {
        let id = uuid::Uuid::new_v4().to_string();
        store::execute_remote_input(&pool, &ws, actor, &id, &command).await
    };
    write("owner", json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":{
        "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
        "ac":{"title":"Criteria","role":"ac","status":"children","parents":["requirement"]},
        "test":{"title":"Tests","role":"test","status":"own","parents":["ac"],"title_preview_length":400}
    }})).await?;
    write("bot", json!({"op":"save_object","kind":"requirement","id":"r","revision":0,"parent":null,"data":{"title":"R","owner":"owner","body":"The rule"},"reason":"Create"})).await?;
    write("bot", json!({"op":"save_object","kind":"ac","id":"a","revision":0,"parent":"requirement:r","data":{"title":"A","owner":"owner"},"reason":"Create"})).await?;
    // Each test of A is a test node under it.
    for name in tests {
        write("bot", json!({"op":"save_object","kind":"test","revision":0,"parent":A,"data":{"title":name,"owner":"owner"},"reason":"Bind"})).await?;
    }
    let dir = std::env::temp_dir().join(format!("joppa-record-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let token = || uuid::Uuid::new_v4().simple().to_string();
    let (owner, bot, alex, outsider) = (token(), token(), token(), token());
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({ owner.clone(): "owner", bot.clone(): "bot", alex.clone(): "alex", outsider.clone(): "outsider" }).to_string(),
    )?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let env = env.map(|env| vars(&root, env)).unwrap_or_default();
    let server = Some(serve::start_with(&url, &tokens, port, &env).await?);
    Ok(World {
        server,
        url,
        port,
        dir,
        env,
        root,
        ws,
        owner,
        bot,
        alex,
        outsider,
        pool,
        http: reqwest::Client::new(),
    })
}

impl World {
    /// Stop the server. The database keeps its rows.
    pub fn stop(&mut self) {
        self.server = None;
    }
    /// Start the server again on the same port, database and tokens.
    pub async fn start(&mut self) -> Result<()> {
        let tokens = self.dir.join("tokens.json");
        self.server = Some(serve::start_with(&self.url, &tokens, self.port, &self.env).await?);
        Ok(())
    }
    /// A second server, with the same database and tokens, as `world_env` starts it. Gives its root.
    pub async fn spare(&self, env: &[(&str, String)]) -> Result<(serve::Server, String)> {
        let port = std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?;
        let root = format!("http://{port}");
        let port = port.port();
        let tokens = self.dir.join("tokens.json");
        let server = serve::start_with(&self.url, &tokens, port, &vars(&root, env)).await?;
        Ok((server, root))
    }
    /// One MCP tool call. Without a token, the call has no session. Gives the HTTP code and body.
    pub async fn mcp_raw(
        &self,
        token: Option<&str>,
        tool: &str,
        args: Value,
    ) -> Result<(u16, Value)> {
        let mut request = self.http.post(format!("{}/mcp", self.root)).json(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}}),
        );
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let reply = request.send().await?;
        Ok((reply.status().as_u16(), reply.json().await?))
    }
    /// One MCP tool call with a token. A refusal gives Err with the exact error text.
    pub async fn mcp(
        &self,
        token: &str,
        tool: &str,
        mut args: Value,
    ) -> Result<Result<Value, String>> {
        args["workspace"] = json!(self.ws);
        let (code, reply) = self.mcp_raw(Some(token), tool, args).await?;
        ensure!(code == 200, "{tool}: {code} {reply}");
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .with_context(|| format!("MCP reply: {reply}"))?;
        Ok(if reply["result"]["isError"] == true {
            Err(text.to_owned())
        } else {
            Ok(serde_json::from_str(text)?)
        })
    }
    /// Record results for one commit through joppa_record_results.
    pub async fn record(
        &self,
        token: &str,
        commit: &str,
        results: Value,
    ) -> Result<Result<Value, String>> {
        self.mcp(
            token,
            "joppa_record_results",
            json!({"commit":commit,"results":results}),
        )
        .await
    }
    /// A record that must succeed.
    pub async fn recorded(&self, commit: &str, results: Value) -> Result<Value> {
        self.record(&self.bot, commit, results)
            .await?
            .map_err(|e| anyhow::anyhow!("record {commit}: {e}"))
    }
    /// One command through joppa_command. A refusal gives Err with the exact error text.
    pub async fn command(&self, token: &str, command: Value) -> Result<Result<Value, String>> {
        let request_id = uuid::Uuid::new_v4().to_string();
        self.mcp(
            token,
            "joppa_command",
            json!({"request_id":request_id,"command":command}),
        )
        .await
    }
    /// The member name of a token of this world. The outsider token gives "outsider".
    pub fn member(&self, token: &str) -> &'static str {
        [
            (&self.owner, "owner"),
            (&self.bot, "bot"),
            (&self.alex, "alex"),
        ]
        .into_iter()
        .find(|(t, _)| t.as_str() == token)
        .map_or("outsider", |(_, name)| name)
    }
    /// The detailed test status of A for the member of `token`, from store::test_status.
    /// HTTP and MCP have no other read of this status. A non-member gets the access error.
    pub async fn a_status(&self, token: &str) -> Result<Value> {
        store::test_status(&self.pool, &self.ws, self.member(token), A).await
    }
    /// The journal position of the newest entry that member alex reads in joppa_history.
    pub async fn position(&self) -> Result<Value> {
        let newest = json!({"order":"newest","limit":1});
        let page = self.mcp(&self.alex, "joppa_history", newest).await?;
        let page = page.map_err(|e| anyhow::anyhow!("joppa_history: {e}"))?;
        let position = page["items"][0]["position"].clone();
        ensure!(position.is_u64(), "{page}");
        Ok(position)
    }
    /// The current document of an object through joppa_objects.
    pub async fn object(&self, address: &str) -> Result<Value> {
        self.mcp(&self.bot, "joppa_objects", json!({"address":address}))
            .await?
            .map_err(|e| anyhow::anyhow!("joppa_objects: {e}"))
    }
    /// The database clock that supplies journal timestamps.
    pub async fn clock(&self) -> Result<String> {
        let sql = "SELECT clock_timestamp()::text";
        Ok(self.pool.get().await?.query_one(sql, &[]).await?.get(0))
    }
}

/// The row of one test in a status reply.
pub fn test<'a>(status: &'a Value, address: &str) -> &'a Value {
    status["tests"]
        .as_array()
        .and_then(|tests| tests.iter().find(|t| t["address"] == address))
        .unwrap_or(&Value::Null)
}
