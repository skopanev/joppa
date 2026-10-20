//! The workspace timeline over HTTP and MCP against a real server.
use anyhow::{Result, ensure};
use joppa::{sessions, store};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Workspace W with the members bot (agent), pat (person) and runner (agent, uploads reports).
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
    "domain":{"title":"Domains","role":"group","root":true,"parents":[]},
    "capability":{"title":"Capabilities","role":"group","parents":["domain"]},
    "requirement":{"title":"Requirements","role":"requirement","parents":["capability"]},
    "ac":{"title":"Criteria","role":"ac","parents":["requirement"]},
    "signal":{"title":"Signals","role":"document","root":true,"parents":["*"],"inbox":true}
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
    for (id, role) in [("bot", "agent"), ("pat", "human"), ("runner", "agent")] {
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
        ["owner", "bot", "pat", "runner"]
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
            .env_remove("DEEPINFRA_API_KEY")
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
    async fn upload(&self, actor: &str, commit: &str) -> Result<Value> {
        let reply = self
            .http
            .post(format!("{}/api/workspaces/{}/reports", self.root, self.ws))
            .bearer_auth(self.token(actor))
            .json(&json!({"stamp":{"commit":commit,"order":1,"branch":"master","default_branch":true,"dirty":false,"full":true},
                "junit":"<testsuites><testsuite name=\"checks.cjs\"><testcase name=\"one\"/></testsuite></testsuites>"}))
            .send()
            .await?;
        let code = reply.status();
        let body: Value = reply.json().await?;
        ensure!(code.is_success(), "upload: {code} {body}");
        Ok(body)
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

/// The ac1 fixture: the tree before p, then the five changes after p.
/// Returns p and the report id.
async fn five_changes(w: &World) -> Result<(i64, i64)> {
    w.save("bot", "domain:d", 0, None, "Add the domain").await?;
    w.save(
        "bot",
        "capability:c1",
        0,
        Some("domain:d"),
        "Add the first capability",
    )
    .await?;
    w.save(
        "bot",
        "capability:c2",
        0,
        Some("domain:d"),
        "Add the second capability",
    )
    .await?;
    w.save(
        "bot",
        "requirement:q",
        0,
        Some("capability:c1"),
        "Add the requirement of A",
    )
    .await?;
    w.save("bot", "ac:a", 0, Some("requirement:q"), "Add criterion A")
        .await?;
    w.save("bot", "signal:n", 0, None, "Add signal N").await?;
    let a = w.get("pat", "/objects?address=ac:a").await?;
    w.command(
        "pat",
        json!({"op":"confirm","req":a["process"]["req"],"revision":a["process"]["revision"]}),
    )
    .await?;
    let p = w.head().await?;
    w.save(
        "bot",
        "requirement:r",
        0,
        Some("capability:c1"),
        "Add requirement R",
    )
    .await?;
    w.save(
        "pat",
        "requirement:r",
        1,
        Some("capability:c2"),
        "Move R to the second capability",
    )
    .await?;
    w.command("pat", json!({"op":"archive_object","target":"signal:n","revision":1,"reason":"Signal N is handled"})).await?;
    let a = w.get("pat", "/objects?address=ac:a").await?;
    w.command("pat", json!({"op":"assess","req":a["process"]["req"],"revision":a["process"]["revision"],"ac":a["process"]["criterion"],
        "assessment":{"ready":true,"reason":"The scope of A is known"}})).await?;
    let report = w.upload("runner", "0123456789abcdef0123").await?;
    Ok((p, report["report"].as_i64().unwrap_or_default()))
}

// workspace-timeline-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_reads_the_five_changes_after_a_position_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let (p, report) = five_changes(&w).await?;
    let http = w.get("bot", &format!("/timeline?after={p}")).await?;
    let mcp = w
        .mcp(
            w.token("bot"),
            "joppa_timeline",
            json!({"workspace":w.ws,"after":p}),
        )
        .await?;
    ensure!(http == mcp, "HTTP {http}\nMCP {mcp}");
    ensure!(http["next_cursor"].is_null(), "{http}");
    let items = http["items"].as_array().unwrap();
    ensure!(items.len() == 5, "{http}");
    let mut last = p;
    for item in items {
        for field in ["position", "at", "actor", "address", "change", "reason"] {
            ensure!(
                item.get(field).is_some_and(|v| !v.is_null() && v != ""),
                "{field} is missing: {item}"
            );
        }
        let position = item["position"].as_i64().unwrap();
        ensure!(position > last, "{http}");
        last = position;
    }
    let column = |field: &str| items.iter().map(|i| i[field].clone()).collect::<Vec<_>>();
    ensure!(
        column("change") == ["save", "move", "archive", "readiness", "report"],
        "{http}"
    );
    ensure!(
        column("actor") == ["bot", "pat", "pat", "pat", "runner"],
        "{http}"
    );
    ensure!(
        column("actor_type") == ["agent", "person", "person", "person", "agent"],
        "{http}"
    );
    ensure!(
        column("address")
            == [
                json!("requirement:r"),
                json!("requirement:r"),
                json!("signal:n"),
                json!("ac:a"),
                json!(format!("report/{report}"))
            ],
        "{http}"
    );
    ensure!(
        column("reason")[..4]
            == [
                "Add requirement R",
                "Move R to the second capability",
                "Signal N is handled",
                "The scope of A is known"
            ],
        "{http}"
    );
    ensure!(
        items[4]["reason"]
            .as_str()
            .is_some_and(|r| r.contains("0123456789ab") && r.contains("master")),
        "{http}"
    );
    ensure!(column("revision")[..3] == [1, 2, 2], "{http}");
    // The journal holds exactly these five changes after p.
    ensure!(w.head().await? == last, "{http}");
    Ok(())
}

// workspace-timeline-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_read_only_agent_reads_the_timeline_and_a_stranger_gets_the_absent_workspace_reply()
-> Result<()> {
    let w = world().await?;
    w.save("bot", "domain:d", 0, None, "Add the domain").await?;
    let reader = store::create_agent_token(&w.pool, &w.ws, "owner", "reader", Some("read")).await?
        ["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let read = |ws: String, auth: (&'static str, String)| {
        let w = &w;
        async move {
            let reply = w
                .http
                .get(format!("{}/api/workspaces/{ws}/timeline?after=0", w.root))
                .header(auth.0, auth.1)
                .send()
                .await?;
            Ok::<_, anyhow::Error>((reply.status().as_u16(), reply.text().await?))
        }
    };
    let (code, body) = read(w.ws.clone(), ("authorization", format!("Bearer {reader}"))).await?;
    let timeline: Value = serde_json::from_str(&body)?;
    ensure!(
        code == 200
            && timeline["items"]
                .as_array()
                .is_some_and(|items| items.iter().any(|i| i["address"] == "domain:d")),
        "{code} {body}"
    );
    let mcp = w
        .mcp(&reader, "joppa_timeline", json!({"workspace":w.ws}))
        .await?;
    ensure!(mcp == timeline, "{mcp} / {timeline}");
    let eve = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&w.pool, &eve, "eve", None).await?;
    let cookie = ("cookie", format!("joppa_session={eve}"));
    let existing = read(w.ws.clone(), cookie.clone()).await?;
    let absent = read(format!("absent-{}", uuid::Uuid::new_v4().simple()), cookie).await?;
    ensure!(
        existing.0 == 404 && existing.1 == r#"{"error":"Workspace not found"}"#,
        "{existing:?}"
    );
    ensure!(existing == absent, "{existing:?} vs {absent:?}");
    Ok(())
}

// workspace-timeline-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_follows_the_cursor_over_250_changes_and_gets_each_one_once_in_journal_order()
-> Result<()> {
    let w = world().await?;
    w.save("bot", "domain:d", 0, None, "Add the domain").await?;
    let p = w.head().await?;
    // Each save changes the body: an unchanged save adds no journal event.
    for revision in 0..250 {
        w.command("bot", json!({"op":"save_object","kind":"signal","id":"s","revision":revision,"parent":null,
            "data":{"title":"Signal S","owner":"pat","body":format!("Source {}", revision + 1)},"reason":"Save the next source"})).await?;
    }
    let journal: Vec<i64> = w
        .pool
        .get()
        .await?
        .query(
            "SELECT position FROM joppa.events WHERE workspace=$1 AND position>$2 ORDER BY position",
            &[&w.ws, &p],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    ensure!(journal.len() == 250, "{}", journal.len());
    let mut positions = vec![];
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let path = match &cursor {
            Some(c) => format!("/timeline?after={p}&cursor={c}"),
            None => format!("/timeline?after={p}"),
        };
        let page = w.get("bot", &path).await?;
        pages += 1;
        ensure!(pages <= 10, "the cursor does not end");
        for item in page["items"].as_array().unwrap() {
            ensure!(item["change"] == "save", "{item}");
            positions.push(item["position"].as_i64().unwrap());
        }
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => {
                ensure!(page["next_cursor"].is_null(), "{page}");
                break;
            }
        }
    }
    ensure!(pages == 5, "{pages} pages of 50");
    ensure!(
        positions.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "a gap or a duplicate: {positions:?}"
    );
    ensure!(positions == journal, "{positions:?} / {journal:?}");
    // MCP follows the same cursor with pages of 100.
    let mut walked = vec![];
    let mut arguments = json!({"workspace":w.ws,"after":p,"limit":100});
    for page in 1.. {
        ensure!(page <= 4, "the MCP cursor does not end");
        let reply = w
            .mcp(w.token("bot"), "joppa_timeline", arguments.clone())
            .await?;
        walked.extend(
            reply["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["position"].as_i64().unwrap()),
        );
        if reply["next_cursor"].is_null() {
            ensure!(page == 3, "{page} MCP pages of 100");
            break;
        }
        arguments["cursor"] = reply["next_cursor"].clone();
    }
    ensure!(walked == journal, "{walked:?} / {journal:?}");
    Ok(())
}
