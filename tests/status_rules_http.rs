//! Status rules of the tree schema over a real server: each type gets its tone from tests,
//! from its children or not at all, and HTTP and MCP give the same tone.
use anyhow::{Context, Result, ensure};
use joppa::{commands::Command, store};
use serde_json::{Value, json};
use std::process::{Child, Command as Process, Stdio};
use std::time::Duration;

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// One workspace with owner S (token `owner`), agent bot and member alex, on its own server.
struct World {
    _server: Server,
    _dir: std::path::PathBuf,
    base: String,
    root: String,
    owner: String,
    bot: String,
    alex: String,
    http: reqwest::Client,
    order: std::cell::Cell<i64>,
}

const SUITE: &str = "tests/rules.test.ts > rules";

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
    let ws = format!("status-rules-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Status rules"}),
        json!({"op":"member","id":"bot","role":"agent"}),
        json!({"op":"member","id":"alex","role":"human"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Command = serde_json::from_value(op)?;
        store::execute(&pool, &ws, "owner", &format!("setup-{i}"), &op).await?;
    }
    let dir = std::env::temp_dir().join(format!("joppa-rules-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let (owner, bot, alex) = (
        uuid::Uuid::new_v4().simple().to_string(),
        uuid::Uuid::new_v4().simple().to_string(),
        uuid::Uuid::new_v4().simple().to_string(),
    );
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
        Process::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
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
            return Ok(World {
                _server: server,
                _dir: dir,
                base: format!("{root}/api/workspaces/{ws}"),
                root,
                owner,
                bot,
                alex,
                http: reqwest::Client::new(),
                order: std::cell::Cell::new(0),
            });
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}

impl World {
    async fn try_command(&self, token: &str, command: Value) -> Result<(u16, Value)> {
        let reply = self
            .http
            .post(format!("{}/commands", self.base))
            .bearer_auth(token)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
            .send()
            .await?;
        Ok((reply.status().as_u16(), reply.json().await?))
    }
    async fn command(&self, token: &str, command: Value) -> Result<Value> {
        let (code, reply) = self.try_command(token, command.clone()).await?;
        ensure!(code == 200, "{command}: {code} {reply}");
        Ok(reply)
    }
    async fn tree(&self) -> Result<Value> {
        Ok(self
            .http
            .get(format!("{}/tree", self.base))
            .bearer_auth(&self.owner)
            .send()
            .await?
            .json()
            .await?)
    }
    async fn object(&self, address: &str) -> Result<Value> {
        Ok(self
            .http
            .get(format!("{}/objects?address={address}", self.base))
            .bearer_auth(&self.alex)
            .send()
            .await?
            .json()
            .await?)
    }
    async fn mcp_object(&self, address: &str) -> Result<Value> {
        let reply: Value = self
            .http
            .post(format!("{}/mcp", self.root))
            .bearer_auth(&self.alex)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":self.base.rsplit('/').next().unwrap(),"address":address}}}))
            .send()
            .await?
            .json()
            .await?;
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .with_context(|| format!("MCP reply: {reply}"))?;
        Ok(serde_json::from_str(text)?)
    }
    /// The tone of each address over HTTP; MCP must give the same tone.
    async fn tones(&self, addresses: &[&str]) -> Result<Vec<String>> {
        let mut tones = vec![];
        for address in addresses {
            let http = self.object(address).await?;
            let mcp = self.mcp_object(address).await?;
            let tone = http["signal"]["tone"].as_str().unwrap_or("").to_owned();
            ensure!(
                mcp["signal"]["tone"] == tone.as_str(),
                "{address}: HTTP {tone}, MCP {}",
                mcp["signal"]["tone"]
            );
            tones.push(tone);
        }
        Ok(tones)
    }
    /// A clean full report from bot with each test as pass or fail.
    async fn report(&self, cases: &[(&str, &str)]) -> Result<()> {
        self.order.set(self.order.get() + 1);
        let rows: String = cases
            .iter()
            .map(|(name, status)| match *status {
                "fail" => {
                    format!(r#"<testcase name="{name}"><failure message="boom"/></testcase>"#)
                }
                _ => format!(r#"<testcase name="{name}" />"#),
            })
            .collect();
        let junit = format!(
            r#"<testsuites><testsuite name="tests/rules.test.ts"><testsuite name="rules">{rows}</testsuite></testsuite></testsuites>"#
        );
        let order = self.order.get();
        let stamp = json!({"commit":format!("c{order:06}"),"order":order,"branch":"master","default_branch":true,"dirty":false,"full":true});
        let reply = self
            .http
            .post(format!("{}/reports", self.base))
            .bearer_auth(&self.bot)
            .json(&json!({"stamp": stamp, "junit": junit}))
            .send()
            .await?;
        ensure!(
            reply.status().is_success(),
            "upload: {}",
            reply.text().await?
        );
        Ok(())
    }
    async fn save(&self, kind: &str, id: &str, parent: Option<&str>) -> Result<()> {
        self.save_for(kind, id, parent, None).await
    }
    /// Save a node; a type that requires consumers names consumer `person`.
    async fn save_for(
        &self,
        kind: &str,
        id: &str,
        parent: Option<&str>,
        consumer: Option<&str>,
    ) -> Result<()> {
        let mut data = json!({"title":id,"owner":"owner","body":"The contract","description":"Given: a node.\nWhen: alex reads it.\nThen: it has a tone.\nEvidence: the tone."});
        if let Some(consumer) = consumer {
            data["consumers"] = json!([consumer]);
        }
        self.command(&self.owner, json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"reason":"Build the tree"})).await?;
        Ok(())
    }
    async fn bind(&self, address: &str, tests: &[&str]) -> Result<()> {
        let tests: Vec<String> = tests.iter().map(|t| format!("{SUITE} > {t}")).collect();
        self.command(
            &self.owner,
            json!({"op":"set_ac_tests","target":address,"tests":tests,"reason":"Bind the test"}),
        )
        .await?;
        Ok(())
    }
    /// Save the schema of ac1: area and rule climb from children, check takes its tests,
    /// memo has no status field. Task may sit under a check.
    async fn rules_schema(&self, area_order: Value) -> Result<Value> {
        let tree = self.tree().await?;
        let mut types = tree["schema"]["types"].clone();
        types["area"] = json!({"title":"Areas","role":"group","root":true,"parents":["area"],"status":"children","status_order":area_order});
        types["rule"] =
            json!({"title":"Rules","role":"requirement","parents":["area"],"status":"children"});
        types["check"] = json!({"title":"Checks","role":"ac","parents":["rule"],"status":"tests"});
        types["memo"] = json!({"title":"Memos","role":"document","parents":["rule"]});
        let task = types
            .as_object()
            .and_then(|t| t.iter().find(|(_, v)| v["role"] == "task"))
            .map(|(k, _)| k.clone())
            .context("task type")?;
        types[&task]["parents"]
            .as_array_mut()
            .context("task parents")?
            .push(json!("check"));
        self.command(&self.owner, json!({"op":"set_tree_schema","revision":tree["current_revision"],"types":types,"reason":"Set the status rules"})).await?;
        Ok(json!(task))
    }
    /// Task T under a check, claimed and blocked by bot.
    async fn blocked_task(&self, task_kind: &str, check: &str) -> Result<()> {
        let id = format!("t-{}", check.replace(':', "-"));
        let parent = json!(check);
        self.command(&self.owner, json!({"op":"save_object","kind":task_kind,"id":id,"revision":0,"parent":parent,"data":{"title":"Implement","owner":"owner","contract":"Implement and check the contract","purpose":"implementation"},"reason":"Create work"})).await?;
        let criterion = self.object(check).await?;
        let p = &criterion["process"];
        self.command(&self.owner, json!({"op":"assess","req":p["req"],"revision":p["revision"],"ac":p["criterion"],"assessment":{"ready":true,"reason":"Known scope","recipient":null,"decision":null}})).await?;
        let address = format!("{task_kind}:{id}");
        let revision = self.object(&address).await?["current_revision"].clone();
        self.command(
            &self.bot,
            json!({"op":"claim_task","id":address,"revision":revision}),
        )
        .await?;
        let revision = self.object(&address).await?["current_revision"].clone();
        self.command(&self.bot, json!({"op":"block_task","id":address,"revision":revision,"reason":"The test fails","evidence":"test a failed"})).await?;
        ensure!(
            self.object(&address).await?["process"]["blocked"].is_object(),
            "Task is not blocked"
        );
        Ok(())
    }
}

// type-status-rules-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn type_status_rules_ac1_the_owner_saves_a_status_rule_for_each_type() -> Result<()> {
    let w = world().await?;
    let before = w.tree().await?["current_revision"]
        .as_u64()
        .context("revision")?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    let tree = w.tree().await?;
    let revision = tree["current_revision"].as_u64().context("revision")?;
    ensure!(revision == before + 1, "{before} -> {revision}");
    let types = &tree["schema"]["types"];
    ensure!(
        types["check"]["status"] == "tests"
            && types["rule"]["status"] == "children"
            && types["area"]["status"] == "children"
            && types["area"]["status_order"] == json!(["red", "grey", "green"])
            && types["memo"].get("status").is_none(),
        "{types}"
    );
    for (field, value, error) in [
        (
            "status",
            json!("parent"),
            "a status rule is tests, children or none",
        ),
        (
            "status_order",
            json!(["red", "blue"]),
            "a status order allows only red, grey and green",
        ),
    ] {
        let mut changed = types.clone();
        changed["area"][field] = value;
        let (code, reply) = w
            .try_command(&w.owner, json!({"op":"set_tree_schema","revision":revision,"types":changed,"reason":"Try a wrong rule"}))
            .await?;
        ensure!(
            code != 200 && reply["error"].as_str().is_some_and(|e| e.contains(error)),
            "{field}: {code} {reply}"
        );
        ensure!(
            w.tree().await?["current_revision"] == revision,
            "{field} changed the schema"
        );
    }
    Ok(())
}

// type-status-rules-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn type_status_rules_ac2_a_node_takes_the_first_status_in_its_order_that_a_child_has()
-> Result<()> {
    let w = world().await?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    w.save("area", "g1", None).await?;
    w.save("area", "g2", Some("area:g1")).await?;
    w.save("rule", "r", Some("area:g2")).await?;
    w.save("check", "a", Some("rule:r")).await?;
    w.save("check", "b", Some("rule:r")).await?;
    let nodes = ["rule:r", "area:g2", "area:g1"];
    // Fixture 1: A is green and B is grey.
    w.bind("check:a", &["a"]).await?;
    w.report(&[("a", "pass")]).await?;
    ensure!(w.tones(&["check:a", "check:b"]).await? == ["green", "grey"]);
    ensure!(
        w.tones(&nodes).await? == ["grey", "grey", "grey"],
        "fixture 1"
    );
    // Fixture 2: B is green.
    w.bind("check:b", &["b"]).await?;
    w.report(&[("a", "pass"), ("b", "pass")]).await?;
    ensure!(
        w.tones(&nodes).await? == ["green", "green", "green"],
        "fixture 2"
    );
    // Fixture 3: B is red.
    w.report(&[("a", "pass"), ("b", "fail")]).await?;
    ensure!(w.tones(&nodes).await? == ["red", "red", "red"], "fixture 3");
    // Fixture 4: A is grey and B is red.
    w.bind("check:a", &[]).await?;
    ensure!(w.tones(&["check:a", "check:b"]).await? == ["grey", "red"]);
    ensure!(w.tones(&nodes).await? == ["red", "red", "red"], "fixture 4");
    // Fixture 5: B is red, R2 has grey check D, and areas put grey before red.
    w.bind("check:a", &["a"]).await?;
    w.report(&[("a", "pass"), ("b", "fail")]).await?;
    w.save("rule", "r2", Some("area:g2")).await?;
    w.save("check", "d", Some("rule:r2")).await?;
    let tree = w.tree().await?;
    let mut types = tree["schema"]["types"].clone();
    types["area"]["status_order"] = json!(["grey", "red", "green"]);
    w.command(&w.owner, json!({"op":"set_tree_schema","revision":tree["current_revision"],"types":types,"reason":"Put grey first"})).await?;
    ensure!(
        w.tones(&["rule:r", "rule:r2", "area:g2", "area:g1"])
            .await?
            == ["red", "grey", "grey", "grey"],
        "fixture 5"
    );
    Ok(())
}

// type-status-rules-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn type_status_rules_ac3_empty_nodes_are_grey_and_uncounted_children_change_nothing()
-> Result<()> {
    let w = world().await?;
    let task = w.rules_schema(json!(["red", "grey", "green"])).await?;
    let task = task.as_str().context("task kind")?;
    w.save("area", "h", None).await?;
    w.save("rule", "q", Some("area:h")).await?;
    w.save("rule", "r", Some("area:h")).await?;
    w.save("check", "a", Some("rule:r")).await?;
    w.save("check", "c", Some("rule:r")).await?;
    w.save("memo", "m", Some("rule:r")).await?;
    w.bind("check:a", &["a"]).await?;
    w.bind("check:c", &["c"]).await?;
    w.report(&[("a", "pass"), ("c", "fail")]).await?;
    w.blocked_task(task, "check:a").await?;
    // Q has no checks: grey. R holds green A, red C and memo M: red.
    ensure!(w.tones(&["rule:q", "check:a", "rule:r"]).await? == ["grey", "green", "red"]);
    // A new area with only an empty rule stays grey.
    w.save("area", "e", None).await?;
    w.save("rule", "q2", Some("area:e")).await?;
    ensure!(w.tones(&["area:e"]).await? == ["grey"]);
    // Fixture 2: C is archived; then a new clean full report passes every test of A.
    let c = w.object("check:c").await?;
    w.command(&w.owner, json!({"op":"archive_object","target":"check:c","revision":c["current_revision"],"reason":"Drop the check"})).await?;
    ensure!(
        w.tones(&["rule:r"]).await? == ["grey"],
        "right after the archive"
    );
    w.report(&[("a", "pass")]).await?;
    ensure!(
        w.tones(&["rule:r"]).await? == ["green"],
        "after the new report"
    );
    Ok(())
}

// type-status-rules-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn type_status_rules_ac5_a_schema_without_status_rules_uses_the_defaults() -> Result<()> {
    let w = world().await?;
    let tree = w.tree().await?;
    let types = &tree["schema"]["types"];
    ensure!(
        types
            .as_object()
            .is_some_and(|t| t.values().all(|v| v.get("status").is_none())),
        "{types}"
    );
    let kind = |role: &str| -> Result<String> {
        types
            .as_object()
            .and_then(|t| t.iter().find(|(_, v)| v["role"] == role))
            .map(|(k, _)| k.clone())
            .with_context(|| format!("{role} type"))
    };
    let (requirement, criterion, task) = (kind("requirement")?, kind("ac")?, kind("task")?);
    w.save("consumer", "person", None).await?;
    w.save("domain", "d", None).await?;
    w.save("capability", "k", Some("domain:d")).await?;
    w.save_for(&requirement, "r", Some("capability:k"), Some("person"))
        .await?;
    let r = format!("{requirement}:r");
    w.save_for(&criterion, "a", Some(&r), Some("person"))
        .await?;
    w.save_for(&criterion, "b", Some(&r), Some("person"))
        .await?;
    let (a, b) = (format!("{criterion}:a"), format!("{criterion}:b"));
    // B has no test yet, so it is grey.
    w.bind(&a, &["a"]).await?;
    w.report(&[("a", "pass")]).await?;
    w.blocked_task(&task, &a).await?;
    let nodes = [
        a.as_str(),
        b.as_str(),
        r.as_str(),
        "capability:k",
        "domain:d",
    ];
    let tones = w.tones(&nodes).await?;
    ensure!(
        tones == ["green", "grey", "grey", "grey", "grey"],
        "{tones:?}"
    );
    // B turns green.
    w.bind(&b, &["b"]).await?;
    w.report(&[("a", "pass"), ("b", "pass")]).await?;
    let tones = w.tones(&nodes).await?;
    ensure!(
        tones == ["green", "green", "green", "green", "green"],
        "{tones:?}"
    );
    ensure!(
        w.tree().await?["current_revision"] == tree["current_revision"],
        "the reads changed the schema"
    );
    Ok(())
}
