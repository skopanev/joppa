//! Status rules of the tree schema over a real server: each type gets its tone from tests,
//! from its children or not at all, and HTTP and MCP give the same tone.
use anyhow::{Context, Result, ensure};
use joppa::{commands::Command, store};
use serde_json::{Value, json};
use std::process::{Child, Command as Process, Stdio};
use std::time::Duration;

#[path = "common/record_http.rs"]
mod record_http;

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
        json!({"op":"enroll_service","id":"bot","role":"agent"}),
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
            .env("DEEPINFRA_API_KEY", "test-only-key")
            .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
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
    /// A record from bot with each test as pass or fail, on a new commit.
    async fn report(&self, cases: &[(&str, &str)]) -> Result<()> {
        self.order.set(self.order.get() + 1);
        let order = self.order.get();
        let results = record_http::cases(SUITE, cases);
        let ws = self.base.rsplit('/').next().unwrap();
        let commit = format!("c{order:06}");
        record_http::recorded(&self.http, &self.root, ws, &self.bot, &commit, results).await?;
        Ok(())
    }
    async fn save(&self, kind: &str, id: &str, parent: Option<&str>) -> Result<()> {
        self.save_for(kind, id, parent, None).await
    }
    /// Save a node; a type that requires consumers names consumer `person`. A configured
    /// tree reads the list `consumers`, the built-in tree the single `consumer`.
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
            data["consumer"] = json!(consumer);
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
        types["memo"] =
            json!({"title":"Memos","role":"document","status":"none","parents":["rule"]});
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
    // The MCP tool joppa_tree shows the same revision and the same saved rules.
    let mcp = w.mcp(&w.owner, "joppa_tree", json!({})).await?;
    ensure!(
        mcp["current_revision"] == tree["current_revision"] && mcp["schema"]["types"] == *types,
        "{mcp}"
    );
    ensure!(
        types["check"]["status"] == "tests"
            && types["rule"]["status"] == "children"
            && types["area"]["status"] == "children"
            && types["area"]["status_order"] == json!(["red", "grey", "green"])
            && types["memo"]["status"] == "none",
        "{types}"
    );
    for (field, value, error) in [
        (
            "status",
            json!("parent"),
            "a status rule is tests, children, links or none",
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
        ensure!(
            w.mcp(&w.owner, "joppa_tree", json!({})).await?["current_revision"] == revision,
            "{field} changed the schema over MCP"
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
    // Fixture 2: C is archived; then a new record passes every test of A.
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
    // The schema of a new workspace names the rule of each type: the rules that the
    // roles gave before.
    ensure!(
        types.as_object().is_some_and(|t| t.values().all(|v| {
            let rule = match v["role"].as_str() {
                Some("ac") => "tests",
                Some("group" | "requirement") => "children",
                _ => "none",
            };
            v["status"] == rule
        })),
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

impl World {
    async fn mcp(&self, token: &str, tool: &str, mut arguments: Value) -> Result<Value> {
        arguments["workspace"] = json!(self.base.rsplit('/').next().unwrap());
        let reply: Value = self
            .http
            .post(format!("{}/mcp", self.root))
            .bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}}))
            .send()
            .await?
            .json()
            .await?;
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .with_context(|| format!("MCP reply: {reply}"))?;
        ensure!(reply["result"]["isError"] != true, "{tool}: {text}");
        Ok(serde_json::from_str(text)?)
    }
    /// Mark a criterion ready for work, as its owner.
    async fn ready(&self, criterion: &str) -> Result<()> {
        let p = self.object(criterion).await?["process"].clone();
        self.command(&self.owner, json!({"op":"assess","req":p["req"],"revision":p["revision"],"ac":p["criterion"],"assessment":{"ready":true,"reason":"Known scope","recipient":null,"decision":null}})).await?;
        Ok(())
    }
    async fn set_priority(
        &self,
        token: &str,
        target: &str,
        priority: &str,
    ) -> Result<(u16, Value)> {
        self.try_command(
            token,
            json!({"op":"set_priority","target":target,"priority":priority}),
        )
        .await
    }
    /// The addresses of the unstarted list that start with `prefix`, in list order.
    async fn unstarted(&self, prefix: &str) -> Result<Vec<String>> {
        let list = self
            .mcp(
                &self.bot,
                "joppa_objects",
                json!({"work_filter":"unstarted","limit":100}),
            )
            .await?;
        Ok(list["items"]
            .as_array()
            .context("items")?
            .iter()
            .filter_map(|i| i["address"].as_str())
            .filter(|a| a.starts_with(prefix))
            .map(String::from)
            .collect())
    }
}

// work-priority-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn work_priority_ac1_setting_the_priority_resets_nothing() -> Result<()> {
    let w = world().await?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    w.save("area", "d", None).await?;
    w.save("rule", "r", Some("area:d")).await?;
    w.save("check", "a", Some("rule:r")).await?;
    w.bind("check:a", &["a"]).await?;
    w.report(&[("a", "pass")]).await?;
    w.ready("check:a").await?;
    let before = w.object("check:a").await?;
    ensure!(
        before["process"]["priority"] == "p2",
        "{}",
        before["process"]
    );
    let (code, reply) = w.set_priority(&w.alex, "check:a", "p0").await?;
    ensure!(code == 200, "{code} {reply}");
    let http: Value = w
        .http
        .get(format!("{}/objects?address=check:a", w.base))
        .bearer_auth(&w.bot)
        .send()
        .await?
        .json()
        .await?;
    let mcp = w
        .mcp(&w.bot, "joppa_objects", json!({"address":"check:a"}))
        .await?;
    for read in [&http, &mcp] {
        ensure!(
            read["process"]["priority"] == "p0"
                && read["process"]["assessment"]["state"] == "ready"
                && read["current_revision"] == before["current_revision"],
            "{read}"
        );
    }
    let tests: Value = w
        .http
        .get(format!("{}/tests?ac=check:a", w.base))
        .bearer_auth(&w.bot)
        .send()
        .await?
        .json()
        .await?;
    ensure!(tests["status"] == "green", "{tests}");
    Ok(())
}

// work-priority-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn work_priority_ac2_work_sorts_by_the_priority_chain_from_the_top() -> Result<()> {
    let w = world().await?;
    let task = w.rules_schema(json!(["red", "grey", "green"])).await?;
    let task = task.as_str().context("task kind")?;
    w.save("area", "d1", None).await?;
    w.save("area", "d2", None).await?;
    w.save("rule", "r1", Some("area:d1")).await?;
    w.save("rule", "r2", Some("area:d2")).await?;
    w.save("check", "a1", Some("rule:r1")).await?;
    w.save("check", "a2", Some("rule:r2")).await?;
    w.save("check", "a3", Some("rule:r2")).await?;
    for criterion in ["check:a1", "check:a2", "check:a3"] {
        w.ready(criterion).await?;
    }
    for (target, value) in [
        ("area:d1", "p1"),
        ("area:d2", "p0"),
        ("check:a1", "p0"),
        ("check:a2", "p2"),
        ("check:a3", "p0"),
    ] {
        let (code, reply) = w.set_priority(&w.alex, target, value).await?;
        ensure!(code == 200, "{target}: {code} {reply}");
    }
    let list = w.unstarted("check:").await?;
    ensure!(list == ["check:a3", "check:a2", "check:a1"], "{list:?}");
    // Fixture 2: one open Task under each criterion.
    for (id, parent) in [("t1", "check:a1"), ("t2", "check:a2"), ("t3", "check:a3")] {
        w.command(&w.owner, json!({"op":"save_object","kind":task,"id":id,"revision":0,"parent":parent,"data":{"title":id,"owner":"owner","contract":"Implement and check the contract","purpose":"implementation"},"reason":"Create work"})).await?;
    }
    let available = w
        .mcp(
            &w.bot,
            "joppa_tasks",
            json!({"view":"available","limit":100}),
        )
        .await?;
    let order: Vec<&str> = available["tasks"]
        .as_array()
        .context("tasks")?
        .iter()
        .filter_map(|t| t["address"].as_str())
        .collect();
    let expected: Vec<String> = ["t3", "t2", "t1"]
        .iter()
        .map(|id| format!("{task}:{id}"))
        .collect();
    ensure!(order == expected, "{order:?}");
    w.command(&w.bot, json!({"op":"claim_next_task"})).await?;
    let t3 = w.object(&format!("{task}:t3")).await?;
    ensure!(t3["process"]["assignee"] == "bot", "{}", t3["process"]);
    Ok(())
}

// work-priority-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn work_priority_ac3_only_a_value_of_the_configured_set_is_a_priority() -> Result<()> {
    let w = world().await?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    w.save("area", "d", None).await?;
    w.save("rule", "r", Some("area:d")).await?;
    let (code, reply) = w.set_priority(&w.alex, "rule:r", "high").await?;
    ensure!(
        code != 200
            && reply["error"]
                .as_str()
                .is_some_and(|e| e.contains("priority is one of p0, p1, p2")),
        "{code} {reply}"
    );
    ensure!(w.object("rule:r").await?["process"]["priority"] == "p2");
    let (code, reply) = w.set_priority(&w.alex, "rule:r", "p1").await?;
    ensure!(code == 200, "{code} {reply}");
    let mcp = w
        .mcp(&w.alex, "joppa_objects", json!({"address":"rule:r"}))
        .await?;
    ensure!(mcp["process"]["priority"] == "p1", "{}", mcp["process"]);
    Ok(())
}

// work-priority-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn work_priority_ac6_the_owner_sets_the_list_of_priority_values() -> Result<()> {
    let w = world().await?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    let tree = w.tree().await?;
    w.command(&w.owner, json!({"op":"set_tree_schema","revision":tree["current_revision"],"types":tree["schema"]["types"],"priorities":["urgent","normal","later"],"reason":"Use our priority words"})).await?;
    ensure!(w.tree().await?["schema"]["priorities"] == json!(["urgent", "normal", "later"]));
    w.save("area", "d", None).await?;
    w.save("rule", "r", Some("area:d")).await?;
    w.save("check", "a1", Some("rule:r")).await?;
    w.save("check", "a2", Some("rule:r")).await?;
    w.ready("check:a1").await?;
    w.ready("check:a2").await?;
    let (code, reply) = w.set_priority(&w.bot, "check:a2", "urgent").await?;
    ensure!(code == 200, "{code} {reply}");
    let (code, reply) = w.set_priority(&w.bot, "check:a1", "p0").await?;
    ensure!(
        code != 200
            && reply["error"]
                .as_str()
                .is_some_and(|e| e.contains("priority is one of urgent, normal, later")),
        "{code} {reply}"
    );
    ensure!(w.object("check:a2").await?["process"]["priority"] == "urgent");
    ensure!(w.object("check:a1").await?["process"]["priority"] == "later");
    let list = w.unstarted("check:").await?;
    ensure!(list == ["check:a2", "check:a1"], "{list:?}");
    Ok(())
}

// dependencies-before-priority-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn dependencies_before_priority_ac1_a_p0_requirement_waits_for_the_p2_one_it_depends_on()
-> Result<()> {
    let w = world().await?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    w.save("area", "c", None).await?;
    w.save("rule", "a", Some("area:c")).await?;
    w.save("rule", "b", Some("area:c")).await?;
    w.save("check", "a1", Some("rule:a")).await?;
    w.save("check", "b1", Some("rule:b")).await?;
    let b = w.object("rule:b").await?;
    let data = b["document"]["data"].clone();
    w.command(&w.owner, json!({"op":"save_object","kind":"rule","id":"b","revision":b["current_revision"],"parent":"area:c","data":data,"links":[{"target":"rule:a","relation":"depends_on","blocking":true}],"reason":"B needs A first"})).await?;
    w.ready("check:a1").await?;
    w.ready("check:b1").await?;
    for (target, value) in [("rule:b", "p0"), ("rule:a", "p2")] {
        let (code, reply) = w.set_priority(&w.alex, target, value).await?;
        ensure!(code == 200, "{target}: {code} {reply}");
    }
    let list = w.unstarted("check:").await?;
    ensure!(list == ["check:a1"], "{list:?}");
    let b1 = w
        .mcp(&w.bot, "joppa_objects", json!({"address":"check:b1"}))
        .await?;
    ensure!(
        b1["process"]["readiness_blocker"]["target"] == "rule:a",
        "{}",
        b1["process"]
    );
    // A1 turns green: B1 becomes plannable.
    w.bind("check:a1", &["a1"]).await?;
    w.report(&[("a1", "pass")]).await?;
    let list = w.unstarted("check:").await?;
    ensure!(list == ["check:b1"], "{list:?}");
    let b1 = w
        .mcp(&w.bot, "joppa_objects", json!({"address":"check:b1"}))
        .await?;
    ensure!(
        b1["process"]["readiness_blocker"].is_null(),
        "{}",
        b1["process"]
    );
    Ok(())
}

// ready-signal-takes-tasks-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ready_signal_takes_tasks_ac1_a_task_under_a_signal_waits_for_its_ready_mark() -> Result<()>
{
    let w = world().await?;
    let tree = w.tree().await?;
    let mut types = tree["schema"]["types"].clone();
    let kind_of = |role: &str, inbox: bool| -> Result<String> {
        types
            .as_object()
            .and_then(|t| {
                t.iter()
                    .find(|(_, v)| v["role"] == role && (!inbox || v["inbox"] == true))
            })
            .map(|(k, _)| k.clone())
            .with_context(|| format!("{role} type"))
    };
    let (task, signal) = (kind_of("task", false)?, kind_of("document", true)?);
    types[&task]["parents"]
        .as_array_mut()
        .context("task parents")?
        .push(json!(signal));
    w.command(&w.owner, json!({"op":"set_tree_schema","revision":tree["current_revision"],"types":types,"reason":"Allow work under a signal"})).await?;
    w.command(&w.owner, json!({"op":"save_object","kind":signal,"id":"n","revision":0,"parent":null,"data":{"title":"Login fails","owner":"owner","source":"support chat","body":"The login page fails"},"reason":"Record the signal"})).await?;
    let n = format!("{signal}:n");
    let t = format!("{task}:t");
    w.command(&w.owner, json!({"op":"save_object","kind":task,"id":"t","revision":0,"parent":n,"data":{"title":"Fix the login","owner":"owner","contract":"Fix the login page and check it","purpose":"implementation"},"reason":"Create work"})).await?;
    let revision = w.object(&t).await?["current_revision"].clone();
    // The first claim is refused: N has no readiness mark.
    let (code, reply) = w
        .try_command(
            &w.bot,
            json!({"op":"claim_task","id":t,"revision":revision}),
        )
        .await?;
    ensure!(
        code != 200 && reply["error"] == format!("{n} has no readiness assessment"),
        "{code} {reply}"
    );
    ensure!(w.object(&t).await?["process"]["assignee"].is_null());
    // Member P marks N ready for work; the second claim succeeds.
    w.command(
        &w.alex,
        json!({"op":"assess_signal","target":n,"ready":true,"reason":"Clear fix"}),
    )
    .await?;
    let revision = w.object(&t).await?["current_revision"].clone();
    w.command(
        &w.bot,
        json!({"op":"claim_task","id":t,"revision":revision}),
    )
    .await?;
    let claimed = w.object(&t).await?;
    ensure!(
        claimed["process"]["status"] == "in_progress" && claimed["process"]["assignee"] == "bot",
        "{}",
        claimed["process"]
    );
    let read = w.mcp(&w.bot, "joppa_objects", json!({"address":n})).await?;
    ensure!(
        read["signal"]["assessment"]["state"] == "ready"
            && read["signal"]["assessment"]["reason"] == "Clear fix",
        "{}",
        read["signal"]
    );
    let revision = w.object(&t).await?["current_revision"].clone();
    w.command(
        &w.bot,
        json!({"op":"complete_task","commit":"c0ffee0","id":t,"revision":revision,"evidence":"The login page works"}),
    )
    .await?;
    let done = w.object(&t).await?;
    ensure!(
        done["process"]["status"] == "done" && done["process"].get("verification").is_none(),
        "{}",
        done["process"]
    );
    Ok(())
}

// work-priority-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn work_priority_ac5_a_member_sets_the_priority_and_the_timeline_names_it() -> Result<()> {
    // alex is a member with write access and is not the owner.
    let w = world().await?;
    w.rules_schema(json!(["red", "grey", "green"])).await?;
    w.save("area", "d", None).await?;
    w.save("rule", "r", Some("area:d")).await?;
    w.save("check", "a", Some("rule:r")).await?;
    w.ready("check:a").await?;
    ensure!(w.object("check:a").await?["process"]["priority"] == "p2");
    let before: Value = w
        .http
        .get(format!("{}/timeline?order=newest&limit=1", w.base))
        .bearer_auth(&w.alex)
        .send()
        .await?
        .json()
        .await?;
    let after = before["items"][0]["position"]
        .as_u64()
        .with_context(|| format!("timeline: {before}"))?;
    let (code, reply) = w.set_priority(&w.alex, "check:a", "p0").await?;
    ensure!(code == 200, "{code} {reply}");
    let read = w
        .mcp(&w.alex, "joppa_objects", json!({"address":"check:a"}))
        .await?;
    ensure!(read["process"]["priority"] == "p0", "{}", read["process"]);
    let timeline: Value = w
        .http
        .get(format!("{}/timeline?after={after}", w.base))
        .bearer_auth(&w.alex)
        .send()
        .await?
        .json()
        .await?;
    let entries = timeline["items"].as_array().context("items")?;
    ensure!(entries.len() == 1, "{timeline}");
    let entry = &entries[0];
    ensure!(
        entry["actor"] == "alex"
            && entry["actor_type"] == "person"
            && entry["change"] == "priority"
            && entry["address"] == "check:a",
        "{entry}"
    );
    Ok(())
}

// type-status-rules-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn type_status_rules_ac6_a_links_node_takes_the_first_status_that_a_linking_object_has()
-> Result<()> {
    let w = world().await?;
    let task = w.rules_schema(json!(["red", "grey", "green"])).await?;
    let task = task.as_str().context("task kind")?;
    let tree = w.tree().await?;
    let mut types = tree["schema"]["types"].clone();
    types["user"] =
        json!({"title":"Users","role":"document","root":true,"parents":[],"status":"links"});
    w.command(&w.owner, json!({"op":"set_tree_schema","revision":tree["current_revision"],"types":types,"reason":"Add users with a links rule"})).await?;
    ensure!(w.tree().await?["schema"]["types"]["user"]["status"] == "links");
    w.save("user", "u", None).await?;
    w.save("area", "g", None).await?;
    w.save("rule", "r", Some("area:g")).await?;
    let link = json!([{"target":"user:u","relation":"related","blocking":false}]);
    let save_linked = |kind: &str, id: &str, parent: &str, extra: Value| {
        let mut data = json!({"title":id,"owner":"owner","body":"The contract","description":"Given: a node.\nWhen: alex reads it.\nThen: it has a tone.\nEvidence: the tone."});
        for (k, v) in extra.as_object().into_iter().flatten() {
            data[k] = v.clone();
        }
        json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":data,"links":link,"reason":"Link to the user"})
    };
    // Fixture 1: no object links to U, so U is grey.
    ensure!(w.tones(&["user:u"]).await? == ["grey"], "fixture 1");
    // Fixture 2: green check A links to U.
    w.command(&w.owner, save_linked("check", "a", "rule:r", json!({})))
        .await?;
    w.bind("check:a", &["a"]).await?;
    w.report(&[("a", "pass")]).await?;
    ensure!(
        w.tones(&["check:a", "user:u"]).await? == ["green", "green"],
        "fixture 2"
    );
    // Fixture 3: a done Task under A links to U and takes the status of A.
    w.command(
        &w.owner,
        save_linked(
            task,
            "t",
            "check:a",
            json!({"contract":"Implement and check the contract","purpose":"implementation"}),
        ),
    )
    .await?;
    let p = w.object("check:a").await?["process"].clone();
    w.command(&w.owner, json!({"op":"assess","req":p["req"],"revision":p["revision"],"ac":p["criterion"],"assessment":{"ready":true,"reason":"Known scope","recipient":null,"decision":null}})).await?;
    let t = format!("{task}:t");
    let revision = w.object(&t).await?["current_revision"].clone();
    w.command(
        &w.bot,
        json!({"op":"claim_task","id":t,"revision":revision}),
    )
    .await?;
    let revision = w.object(&t).await?["current_revision"].clone();
    w.command(
        &w.bot,
        json!({"op":"complete_task","commit":"c0ffee0","id":t,"revision":revision,"evidence":"Implemented"}),
    )
    .await?;
    ensure!(w.tones(&["user:u"]).await? == ["green"], "fixture 3");
    // Fixture 4: red check B links to U.
    w.command(&w.owner, save_linked("check", "b", "rule:r", json!({})))
        .await?;
    w.bind("check:b", &["b"]).await?;
    w.report(&[("a", "pass"), ("b", "fail")]).await?;
    ensure!(w.tones(&["user:u"]).await? == ["red"], "fixture 4");
    // Fixture 5: B is archived and an open Task links to U, so U is grey.
    let b = w.object("check:b").await?;
    w.command(&w.owner, json!({"op":"archive_object","target":"check:b","revision":b["current_revision"],"reason":"Drop the check"})).await?;
    w.report(&[("a", "pass")]).await?;
    ensure!(
        w.tones(&["user:u"]).await? == ["green"],
        "after the archive"
    );
    w.command(
        &w.owner,
        save_linked(
            task,
            "open",
            "check:a",
            json!({"contract":"Implement the next part","purpose":"implementation"}),
        ),
    )
    .await?;
    ensure!(w.tones(&["user:u"]).await? == ["grey"], "fixture 5");
    // Fixture 6: the open Task is retired, and a retired Task does not count.
    let o = format!("{task}:open");
    let revision = w.object(&o).await?["current_revision"].clone();
    w.command(
        &w.owner,
        json!({"op":"retire_task","id":o,"revision":revision,"reason":"Not needed"}),
    )
    .await?;
    ensure!(w.tones(&["user:u"]).await? == ["green"], "fixture 6");
    Ok(())
}
