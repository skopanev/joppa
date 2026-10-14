//! Test reports over HTTP: a real server, a member token and uploaded JUnit.
//! Each test proves one Evidence criterion through the endpoints, not the status function.
use anyhow::{Result, ensure};
use joppa::{commands::Command as Operation, store};
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

struct World {
    _server: Server,
    _dir: std::path::PathBuf,
    base: String,
    token: String,
    owner: String,
    http: reqwest::Client,
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
        json!({"op":"member","id":"agent","role":"agent"}),
        json!({"op":"consumer","id":"customer","title":"Customer","owner":"owner"}),
        json!({"op":"domain","id":"security","title":"Security","authority":"owner"}),
        json!({"op":"capability","id":"restrict","title":"Restrict access","domain":"security"}),
        json!({"op":"revise","req":"key","expected_revision":0,"capability":"restrict","title":"Key rights","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"key-rights","text":"Only read and verify before launch"}]}),
        json!({"op":"task","req":"key","revision":1,"id":"t1","title":"Build key rights","acs":["key-rights"],"projects":[],"purpose":"implementation","contract":"Implement and provide evidence"}),
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
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({ token.clone(): "agent", owner.clone(): "owner" }).to_string(),
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
                http: reqwest::Client::new(),
            };
            world.list(&[A, B]).await?;
            return Ok(world);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}

impl World {
    async fn list(&self, tests: &[&str]) -> Result<()> {
        let command = json!({"op":"set_ac_tests","target":"ac:key-rights","tests":tests,"reason":"Link the tests"});
        let reply = self
            .http
            .post(format!("{}/commands", self.base))
            .bearer_auth(&self.token)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
            .send()
            .await?;
        ensure!(
            reply.status().is_success(),
            "set_ac_tests: {}",
            reply.text().await?
        );
        Ok(())
    }
    async fn upload(&self, commit: &str, order: i64, full: bool, junit: String) -> Result<()> {
        let stamp = json!({"commit":commit,"order":order,"branch":"master","default_branch":true,"dirty":false,"full":full});
        let reply = self
            .http
            .post(format!("{}/reports", self.base))
            .bearer_auth(&self.token)
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
    /// Post a report with any stamp and token; returns the status code and the reply body.
    async fn raw_upload(
        &self,
        token: Option<&str>,
        stamp: Value,
        junit: String,
    ) -> Result<(u16, Value)> {
        let mut request = self.http.post(format!("{}/reports", self.base));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let reply = request
            .json(&json!({"stamp": stamp, "junit": junit}))
            .send()
            .await?;
        Ok((
            reply.status().as_u16(),
            reply.json().await.unwrap_or(Value::Null),
        ))
    }
    /// skopanev's role in these tests: change the criterion text, which makes a new revision.
    async fn revise_text(&self, expected: u32, text: &str) -> Result<()> {
        let command = json!({"op":"revise","req":"key","expected_revision":expected,"capability":"restrict","title":"Key rights","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Sharper wording","acs":[{"id":"key-rights","text":text}]});
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
    async fn status(&self) -> Result<Value> {
        Ok(self
            .http
            .get(format!("{}/tests?ac=ac:key-rights", self.base))
            .bearer_auth(&self.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}

/// A JUnit report from `bun test` for tests/auth/login.test.ts > login.
fn junit(cases: &[(&str, &str)]) -> String {
    let rows: String = cases
        .iter()
        .map(|(name, status)| match *status {
            "fail" => format!(r#"<testcase name="{name}"><failure message="boom"/></testcase>"#),
            _ => format!(r#"<testcase name="{name}" />"#),
        })
        .collect();
    format!(
        r#"<testsuites><testsuite name="tests/auth/login.test.ts"><testsuite name="login">{rows}</testsuite></testsuite></testsuites>"#
    )
}

async fn ready_for_planning(w: &World) -> Result<Value> {
    for command in [
        json!({"op":"confirm","req":"key","revision":1}),
        json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"Known work scope"}}),
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

async fn planning_batch(w: &World, operations: Value) -> Result<Value> {
    let root = w.base.split("/api/").next().unwrap();
    let workspace = w.base.rsplit('/').next().unwrap();
    Ok(w.http.post(format!("{root}/mcp")).bearer_auth(&w.token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_batch","arguments":{
            "workspace":workspace,"request_id":uuid::Uuid::new_v4().to_string(),"operations":operations}}}))
        .send().await?.json().await?)
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn planning_claim_refuses_junit_green_over_http_and_mcp_without_mutation() -> Result<()> {
    let w = world().await?;
    ready_for_planning(&w).await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "JUnit must be green"
    );
    let before = planning_criterion(&w).await?;
    let command =
        json!({"op":"claim_planning","ac":"ac:key-rights","revision":before["current_revision"]});
    let (code, reply) = post(
        &w,
        &w.token,
        "/commands",
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
    )
    .await?;
    ensure!(
        code == 409 && reply["error"] == "criterion is already verified",
        "{code} {reply}"
    );
    let root = w.base.split("/api/").next().unwrap();
    let workspace = w.base.rsplit('/').next().unwrap();
    let response: Value = w.http.post(format!("{root}/mcp")).bearer_auth(&w.token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{
            "workspace":workspace,"request_id":uuid::Uuid::new_v4().to_string(),"command":command}}}))
        .send().await?.json().await?;
    ensure!(response["result"]["isError"] == true, "{response}");
    ensure!(
        response["result"]["content"][0]["text"] == "criterion is already verified",
        "{response}"
    );
    let after = planning_criterion(&w).await?;
    ensure!(
        after["document"] == before["document"]
            && after["process"] == before["process"]
            && after["position"] == before["position"],
        "refusal changed the criterion or journal position"
    );
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::check_journal(&pool).await?;
    Ok(())
}

/// A planning workspace on the test server: spec:one with criteria a and b.
/// The token member `agent` is P, and `other` is Q.
struct Planning<'a> {
    w: &'a World,
    pool: deadpool_postgres::Pool,
    ws: String,
}

async fn planning(w: &World) -> Result<Planning<'_>> {
    let p = Planning {
        w,
        pool: store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?,
        ws: format!("http-planning-{}", uuid::Uuid::new_v4().simple()),
    };
    store::execute_input(
        &p.pool,
        &p.ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Planning refusals"}),
    )
    .await?;
    for actor in ["agent", "other"] {
        p.write("owner", json!({"op":"member","id":actor,"role":"agent"}))
            .await?;
    }
    let schema = store::tree_schema(&p.pool, &p.ws, "owner", None).await?;
    p.write("owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","parents":["spec"]},
        "work":{"title":"Work","role":"task","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    p.save(
        "spec:one",
        None,
        json!({"title":"One rule","body":"Implement the agreed rule","owner":"owner"}),
        json!([]),
    )
    .await?;
    for ac in ["criterion:a", "criterion:b"] {
        p.save(
            ac,
            Some("spec:one"),
            json!({"title":ac,"owner":"owner"}),
            json!([]),
        )
        .await?;
    }
    Ok(p)
}

impl Planning<'_> {
    async fn write(&self, actor: &str, command: Value) -> Result<Value> {
        store::execute_remote_input(
            &self.pool,
            &self.ws,
            actor,
            &uuid::Uuid::new_v4().to_string(),
            &command,
        )
        .await
    }
    async fn read(&self, address: &str) -> Result<Value> {
        store::read_objects(
            &self.pool,
            &self.ws,
            "owner",
            &store::ObjectScope {
                address: Some(address),
                ..Default::default()
            },
        )
        .await
    }
    async fn revision(&self, address: &str) -> Result<u64> {
        Ok(self.read(address).await?["current_revision"]
            .as_u64()
            .unwrap_or(0))
    }
    async fn save(&self, key: &str, parent: Option<&str>, data: Value, links: Value) -> Result<()> {
        let (kind, id) = key.split_once(':').unwrap();
        let revision = match self.read(key).await {
            Ok(object) => object["current_revision"].as_u64().unwrap_or(0),
            Err(_) => 0,
        };
        self.write("owner", json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"links":links,"reason":"Set the contract"})).await?;
        Ok(())
    }
    /// Confirm `req` and save each assessment.
    async fn confirm(&self, req: &str, assessments: &[(&str, Value)]) -> Result<()> {
        let revision = self.read(req).await?["process"]["revision"].clone();
        self.write(
            "owner",
            json!({"op":"confirm","req":req,"revision":revision}),
        )
        .await?;
        for (ac, assessment) in assessments {
            self.write("owner", json!({"op":"assess","req":req,"revision":revision,"ac":ac,"assessment":assessment})).await?;
        }
        Ok(())
    }
    async fn ready(&self) -> Result<()> {
        let ready = json!({"ready":true,"reason":"Known scope"});
        self.confirm(
            "spec:one",
            &[("criterion:a", ready.clone()), ("criterion:b", ready)],
        )
        .await
    }
    /// Save prerequisite C in spec:two with `description` and keep it assessed ready.
    async fn prerequisite(&self, description: &str) -> Result<u64> {
        if self.revision("spec:two").await.unwrap_or(0) == 0 {
            self.save(
                "spec:two",
                None,
                json!({"title":"Prerequisite","body":"Deliver the prerequisite","owner":"owner"}),
                json!([]),
            )
            .await?;
        }
        self.save(
            "criterion:c",
            Some("spec:two"),
            json!({"title":"criterion:c","owner":"owner","description":description}),
            json!([]),
        )
        .await?;
        self.confirm(
            "spec:two",
            &[("criterion:c", json!({"ready":true,"reason":"Known scope"}))],
        )
        .await?;
        self.revision("criterion:c").await
    }
    /// A depends on C at `revision`.
    async fn depend(&self, revision: u64, blocking: bool) -> Result<()> {
        self.save("criterion:a", Some("spec:one"), json!({"title":"criterion:a","owner":"owner"}),
            json!([{"target":"criterion:c","relation":"depends_on","revision":revision,"blocking":blocking}])).await
    }
    /// T0: an existing Task under A.
    async fn t0(&self) -> Result<()> {
        self.save("work:t0", Some("criterion:a"), json!({"title":"T0","owner":"owner","contract":"Implement this criterion","purpose":"implementation"}), json!([])).await
    }
    async fn claim(&self, actor: &str, ac: &str) -> Result<()> {
        let revision = self.revision(ac).await?;
        self.write(
            actor,
            json!({"op":"claim_planning","ac":ac,"revision":revision}),
        )
        .await?;
        Ok(())
    }
    /// Committed rows of the workspace: documents, revisions, links, process data,
    /// planning claims, receipts and the journal, each in a stable order.
    async fn saved(&self) -> Result<Value> {
        let client = self.pool.get().await?;
        let mut saved = json!({});
        // Table names are fixed literals; SQL cannot bind identifiers.
        for table in [
            "events",
            "objects",
            "object_revisions",
            "object_links",
            "state_entries",
            "state_revisions",
            "board_cache",
            "command_receipts",
        ] {
            let rows: String = client
                .query_one(
                    &format!("SELECT COALESCE(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)),'[]')::text FROM joppa.{table} t WHERE workspace=$1"),
                    &[&self.ws],
                )
                .await?
                .get(0);
            saved[table] = serde_json::from_str(&rows)?;
        }
        let position: i64 = client
            .query_one(
                "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1",
                &[&self.ws],
            )
            .await?
            .get(0);
        saved["position"] = json!(position);
        Ok(saved)
    }
    async fn saved_object(&self, address: &str) -> Result<Value> {
        let saved = self.saved().await?;
        Ok(saved["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["address"] == address)
            .cloned()
            .unwrap_or(Value::Null))
    }
    /// Fixture 1: A is ready and has no planning claim. L (`other`) holds the Task
    /// at revision 4 and blocks it. Returns the position of the block event.
    async fn blocked(&self, task: &str) -> Result<u64> {
        self.ready().await?;
        self.save(task, Some("criterion:a"), json!({"title":"T","owner":"owner","contract":"Implement this criterion","purpose":"implementation"}), json!([])).await?;
        for (op, reason) in [
            ("claim_task", None),
            ("release_task", Some("Return the Task")),
            ("claim_task", None),
        ] {
            let mut command = json!({"op":op,"id":task,"revision":self.revision(task).await?});
            if let Some(reason) = reason {
                command["reason"] = json!(reason);
            }
            self.write("other", command).await?;
        }
        ensure!(self.revision(task).await? == 4);
        let reply = self.write("other", json!({"op":"block_task","id":task,"revision":4,"reason":"Verification failed","evidence":"test x failed on abc123"})).await?;
        reply["position"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("block reply has no position: {reply}"))
    }
    /// P sends the claim through HTTP and then MCP. Each refusal returns `expected`
    /// and leaves the committed workspace rows unchanged.
    async fn refuse(&self, case: &str, ac: &str, revision: u64, expected: &str) -> Result<()> {
        let command = json!({"op":"claim_planning","ac":ac,"revision":revision});
        self.refuse_command(case, command, expected).await
    }
    /// P posts the request body to the HTTP command endpoint.
    async fn http(&self, body: Value) -> Result<(u16, Value)> {
        let root = self.w.base.split("/api/").next().unwrap();
        let reply = self
            .w
            .http
            .post(format!("{root}/api/workspaces/{}/commands", self.ws))
            .bearer_auth(&self.w.token)
            .json(&body)
            .send()
            .await?;
        let code = reply.status().as_u16();
        Ok((code, reply.json().await.unwrap_or(Value::Null)))
    }
    /// P calls one MCP tool in this workspace.
    async fn mcp(&self, tool: &str, mut arguments: Value) -> Result<Value> {
        let root = self.w.base.split("/api/").next().unwrap();
        arguments["workspace"] = json!(self.ws);
        Ok(self.w.http.post(format!("{root}/mcp")).bearer_auth(&self.w.token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}}))
            .send().await?.json().await?)
    }
    /// P sends `command` through HTTP or MCP and gets the result or the error text.
    async fn send(&self, mcp: bool, command: &Value) -> Result<std::result::Result<Value, String>> {
        let request_id = uuid::Uuid::new_v4().to_string();
        if mcp {
            let response = self
                .mcp(
                    "joppa_command",
                    json!({"request_id":request_id,"command":command}),
                )
                .await?;
            let text = response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            return Ok(if response["result"]["isError"] == true {
                Err(text)
            } else {
                Ok(serde_json::from_str(&text)?)
            });
        }
        let (code, reply) = self
            .http(json!({"request_id":request_id,"command":command}))
            .await?;
        Ok(if code == 200 {
            Ok(reply)
        } else {
            Err(format!("{code} {reply}"))
        })
    }
    /// P sends the command through HTTP and then MCP. Each refusal returns `expected`
    /// and leaves the committed workspace rows unchanged.
    async fn refuse_command(&self, case: &str, command: Value, expected: &str) -> Result<()> {
        let before = self.saved().await?;
        let (code, reply) = self
            .http(json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}))
            .await?;
        ensure!(
            (400..500).contains(&code) && reply["error"] == expected,
            "{case} over HTTP: expected {expected:?}, got {code} {reply}"
        );
        ensure!(
            self.saved().await? == before,
            "{case}: the HTTP refusal changed saved state"
        );
        let response = self
            .mcp(
                "joppa_command",
                json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
            )
            .await?;
        ensure!(
            response["result"]["isError"] == true
                && response["result"]["content"][0]["text"] == expected,
            "{case} over MCP: expected {expected:?}, got {response}"
        );
        ensure!(
            self.saved().await? == before,
            "{case}: the MCP refusal changed saved state"
        );
        Ok(())
    }
}

// planning-claim-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn planning_claim_guards_refuse_over_http_and_mcp_without_changing_saved_state() -> Result<()>
{
    let w = world().await?;

    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    p.claim("other", "criterion:a").await?;
    ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "other");
    let revision = p.revision("criterion:a").await?;
    p.refuse(
        "claimed A",
        "criterion:a",
        revision,
        "criterion is already claimed for planning",
    )
    .await?;

    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    p.prerequisite("Given: a ready criterion of another REQ.\nWhen: it is planned.\nThen: it has a plan.\nEvidence: a Task.")
        .await?;
    p.claim("agent", "criterion:a").await?;
    ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "agent");
    let revision = p.revision("criterion:c").await?;
    p.refuse(
        "P holds A and claims C of another REQ",
        "criterion:c",
        revision,
        "agent already holds a planning claim in this workspace",
    )
    .await?;
    let c = p.saved_object("criterion:c").await?;
    ensure!(
        c.is_object() && c["process"]["planning_claim"].is_null(),
        "C must stay unclaimed: {c}"
    );

    let p = planning(&w).await?;
    p.confirm(
        "spec:one",
        &[("criterion:b", json!({"ready":true,"reason":"Known scope"}))],
    )
    .await?;
    let a = p.read("criterion:a").await?;
    ensure!(a["process"]["assessment"]["state"] == "unassessed", "{a}");
    p.refuse(
        "unassessed A",
        "criterion:a",
        a["current_revision"].as_u64().unwrap(),
        "criterion is not ready for planning",
    )
    .await?;

    let p = planning(&w).await?;
    p.confirm(
        "spec:one",
        &[
            (
                "criterion:a",
                json!({"ready":false,"reason":"Unknown input","recipient":"owner"}),
            ),
            ("criterion:b", json!({"ready":true,"reason":"Known scope"})),
        ],
    )
    .await?;
    let a = p.read("criterion:a").await?;
    ensure!(a["process"]["assessment"]["state"] == "waiting", "{a}");
    p.refuse(
        "waiting A",
        "criterion:a",
        a["current_revision"].as_u64().unwrap(),
        "criterion is not ready for planning",
    )
    .await?;

    // A blocks on C at C's current revision; C is assessed ready but not verified.
    let p = planning(&w).await?;
    let c = p.prerequisite("Prerequisite scope").await?;
    p.depend(c, true).await?;
    p.ready().await?;
    let a = p.read("criterion:a").await?;
    ensure!(
        a["process"]["assessment"]["state"] == "ready"
            && a["process"]["status"]["review_required"] == json!([])
            && a["work_ready"] == false
            && p.revision("criterion:c").await? == c,
        "{a}"
    );
    p.refuse(
        "dependency-blocked A",
        "criterion:a",
        a["current_revision"].as_u64().unwrap(),
        "criterion is not ready for planning",
    )
    .await?;

    // A's non-blocking dependency names an older revision of C; C stays assessed ready.
    let p = planning(&w).await?;
    let c = p.prerequisite("Prerequisite scope").await?;
    p.depend(c, false).await?;
    ensure!(p.prerequisite("Sharper prerequisite scope").await? > c);
    p.ready().await?;
    let a = p.read("criterion:a").await?;
    ensure!(
        a["process"]["assessment"]["state"] == "ready"
            && a["process"]["status"]["review_required"] != json!([])
            && a["work_ready"] == true,
        "{a}"
    );
    p.refuse(
        "A needs dependency review",
        "criterion:a",
        a["current_revision"].as_u64().unwrap(),
        "criterion is not ready for planning",
    )
    .await?;

    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    p.write(
        "agent",
        json!({"op":"set_ac_tests","target":"criterion:a","tests":[A,B],"reason":"Link the tests"}),
    )
    .await?;
    let stamp = json!({"commit":"c1c1c1c","order":1,"branch":"master","default_branch":true,"dirty":false,"full":true});
    let root = w.base.split("/api/").next().unwrap();
    let base = format!("{root}/api/workspaces/{}", p.ws);
    w.http
        .post(format!("{base}/reports"))
        .bearer_auth(&w.token)
        .json(&json!({"stamp":stamp,"junit":junit(&[("opens", "pass"), ("refuses", "pass")])}))
        .send()
        .await?
        .error_for_status()?;
    let status: Value = w
        .http
        .get(format!("{base}/tests?ac=criterion:a"))
        .bearer_auth(&w.token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(status["status"] == "green", "JUnit must be green: {status}");
    let revision = p.revision("criterion:a").await?;
    p.refuse(
        "verified A",
        "criterion:a",
        revision,
        "criterion is already verified",
    )
    .await?;

    let p = planning(&w).await?;
    while let n @ ..8 = p.revision("criterion:a").await? {
        p.save(
            "criterion:a",
            Some("spec:one"),
            json!({"title":"criterion:a","owner":"owner","description":format!("Draft {n}")}),
            json!([]),
        )
        .await?;
    }
    p.ready().await?;
    p.t0().await?;
    ensure!(p.revision("criterion:a").await? == 8);
    p.refuse(
        "current revision 8, request revision 7",
        "criterion:a",
        7,
        "stale object revision; read the object again",
    )
    .await?;

    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    ensure!(p.saved_object("criterion:missing").await?.is_null());
    p.refuse("unknown A", "criterion:missing", 1, "unknown criterion")
        .await?;

    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    let revision = p.revision("work:t0").await?;
    ensure!(revision > 0);
    p.refuse(
        "object without the criterion role",
        "work:t0",
        revision,
        "planning requires a criterion",
    )
    .await?;
    store::check_journal(&p.pool).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn planning_claim_uses_the_current_batch_test_list_in_both_directions() -> Result<()> {
    let w = world().await?;
    ready_for_planning(&w).await?;
    w.list(&[A]).await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "fail")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "the stored list must be green"
    );
    let before = planning_criterion(&w).await?;
    let claim =
        json!({"op":"claim_planning","ac":"ac:key-rights","revision":before["current_revision"]});
    let reply = planning_batch(&w, json!([
        {"command":{"op":"set_ac_tests","target":"ac:key-rights","tests":[A,B],"reason":"Include the failing check"}},
        {"command":claim}
    ])).await?;
    ensure!(
        reply["result"]["isError"] != true && reply.get("error").is_none(),
        "current red list must permit planning: {reply}"
    );
    ensure!(
        w.status().await?["status"] == "red",
        "the new list must be red"
    );
    ensure!(
        planning_criterion(&w).await?["process"]["planning_claim"]["actor"] == "agent",
        "claim missing"
    );
    let (code, reply) = post(&w, &w.token, "/commands", json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{
        "op":"release_planning","ac":"ac:key-rights","reason":"Check the opposite batch transition"}})).await?;
    ensure!(code == 200, "{code} {reply}");
    let before = planning_criterion(&w).await?;
    let status_before = w.status().await?;
    let claim =
        json!({"op":"claim_planning","ac":"ac:key-rights","revision":before["current_revision"]});
    let reply = planning_batch(&w, json!([
        {"command":{"op":"set_ac_tests","target":"ac:key-rights","tests":[A],"reason":"Use the passing check"}},
        {"command":claim}
    ])).await?;
    ensure!(
        reply["result"]["isError"] == true
            && reply["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|error| error.ends_with("criterion is already verified")),
        "current green list must refuse planning: {reply}"
    );
    let after = planning_criterion(&w).await?;
    ensure!(
        after["document"] == before["document"]
            && after["process"] == before["process"]
            && after["position"] == before["position"],
        "rejected batch changed the criterion or journal position"
    );
    ensure!(
        w.status().await? == status_before,
        "rejected batch changed the saved test list"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_tree_counts_ready_direct_children_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let command = |value: Value| {
        let w = &w;
        async move {
            let (code, reply) = post(
                w,
                &w.owner,
                "/commands",
                json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":value}),
            )
            .await?;
            ensure!(code == 200, "{code}: {reply}");
            Ok::<_, anyhow::Error>(())
        }
    };
    let tree: Value = w
        .http
        .get(format!("{}/tree", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    let mut types = tree["schema"]["types"].clone();
    for (kind, role, parent, title) in [
        ("bundle", "group", None, "Bundles"),
        ("branch", "group", Some("bundle"), "Branches"),
        ("sheet", "requirement", Some("branch"), "Sheets"),
        ("proof", "ac", Some("sheet"), "Proofs"),
        ("extra", "document", Some("bundle"), "Extra"),
    ] {
        types[kind] = json!({"title":title,"role":role,"root":parent.is_none(),"parents":parent.into_iter().collect::<Vec<_>>()});
    }
    command(json!({"op":"set_tree_schema","revision":tree["current_revision"],"types":types,"reason":"Count configured tree levels"})).await?;
    for (kind, id, parent) in [
        ("bundle", "root", None),
        ("branch", "good", Some("bundle:root")),
        ("branch", "bad", Some("bundle:root")),
        ("branch", "empty", Some("bundle:root")),
        ("sheet", "one", Some("branch:good")),
        ("sheet", "two", Some("branch:good")),
        ("sheet", "three", Some("branch:bad")),
        ("extra", "input", Some("bundle:root")),
    ] {
        command(json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":{"title":id,"body":"The contract","owner":"owner"},"reason":"Create the counted tree"})).await?;
    }
    for (id, parent, test) in [
        ("a", "sheet:one", A),
        ("b", "sheet:one", A),
        ("c", "sheet:two", A),
        ("d", "sheet:three", B),
    ] {
        command(json!({"op":"save_object","kind":"proof","id":id,"revision":0,"parent":parent,"data":{"title":id,"description":"The exact criterion","owner":"owner"},"reason":"Create a configured criterion"})).await?;
        command(json!({"op":"set_ac_tests","target":format!("proof:{id}"),"tests":[test],"reason":"Bind its exact test"})).await?;
    }
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "fail")]),
    )
    .await?;
    for (address, total, passed, failed) in [
        ("bundle:root", 3, 1, 1),
        ("branch:good", 2, 2, 0),
        ("branch:bad", 1, 0, 1),
        ("branch:empty", 0, 0, 0),
        ("sheet:one", 2, 2, 0),
    ] {
        let http: Value = w
            .http
            .get(format!("{}/objects?address={address}", w.base))
            .bearer_auth(&w.owner)
            .send()
            .await?
            .json()
            .await?;
        let root = w.base.split("/api/").next().unwrap();
        let workspace = w.base.rsplit('/').next().unwrap();
        let mcp: Value = w.http.post(format!("{root}/mcp")).bearer_auth(&w.owner).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":workspace,"address":address}}})).send().await?.json().await?;
        let mcp: Value = serde_json::from_str(
            mcp["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        let expected = json!({"total":total,"passed":passed,"failed":failed});
        ensure!(
            http["verification_summary"] == expected && mcp["verification_summary"] == expected,
            "{address}: HTTP {http}, MCP {mcp}"
        );
    }
    let list: Value = w
        .http
        .get(format!("{}/objects?kind=bundle", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    ensure!(
        list["items"][0]["verification_summary"] == json!({"total":3,"passed":1,"failed":1}),
        "{list}"
    );
    command(json!({"op":"archive_object","target":"branch:empty","revision":1,"reason":"Remove an unused branch"})).await?;
    let after: Value = w
        .http
        .get(format!("{}/objects?address=bundle:root", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    ensure!(
        after["verification_summary"] == json!({"total":2,"passed":1,"failed":1}),
        "{after}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn latest_run_decides_by_commit_order_not_upload_time() -> Result<()> {
    // latest-run-decides-ac1: the newer commit uploads first, the older one later.
    let w = world().await?;
    w.upload(
        "c2c2c2c",
        2,
        true,
        junit(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "red"
            && s["tests"][0]["status"] == "fail"
            && s["tests"][0]["commit"] == "c2c2c2c",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_test_absent_from_a_full_run_is_missing() -> Result<()> {
    // status-current-only-ac2: refuses is listed, the full run of C has only opens.
    let w = world().await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    w.upload("c2c2c2c", 2, true, junit(&[("opens", "pass")]))
        .await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "red"
            && s["tests"][1]["status"] == "missing"
            && s["tests"][1]["commit"] == "c2c2c2c",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn every_case_of_a_shared_address_must_pass() -> Result<()> {
    // missing-is-not-pass-ac2: bun 1.4.2 output of test.each with one name and three cases.
    // A green baseline where all three pass, then a later report where exactly one fails.
    let w = world().await?;
    let same = "tests/auth/login.test.ts > login > passkey > same name";
    w.list(&[same]).await?;
    let s = w.status().await?;
    ensure!(
        s["tests_draft"] == false && s["tests"].as_array().map(Vec::len) == Some(1),
        "{s}"
    );
    let bun = |failing: bool| {
        let pass = r#"<testcase name="same name" classname="passkey &gt; login" time="0.000075" file="tests/auth/login.test.ts" line="5" assertions="1" />"#;
        let last = if failing {
            r#"<testcase name="same name" classname="passkey &gt; login" time="0.000794" file="tests/auth/login.test.ts" line="5" assertions="1">
          <failure type="AssertionError" message="Expected: 3&#10;Received: 2&#10;">AssertionError</failure>
        </testcase>"#
        } else {
            pass
        };
        let failures = u8::from(failing);
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="bun test" tests="3" assertions="3" failures="{failures}" skipped="0" time="0.04">
  <testsuite name="tests/auth/login.test.ts" file="tests/auth/login.test.ts" tests="3">
    <testsuite name="login" file="tests/auth/login.test.ts" line="2" tests="3">
      <testsuite name="passkey" file="tests/auth/login.test.ts" line="3" tests="3">
        {pass}
        {pass}
        {last}
      </testsuite>
    </testsuite>
  </testsuite>
</testsuites>"#
        )
    };
    w.upload("c1c1c1c", 1, true, bun(false)).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == same
            && s["tests"][0]["status"] == "pass"
            && s["status"] == "green",
        "{s}"
    );
    w.upload("c2c2c2c", 2, true, bun(true)).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == same
            && s["tests"][0]["status"] == "fail"
            && s["status"] == "red",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_partial_run_updates_only_the_tests_it_ran() -> Result<()> {
    // no-mixed-subjects-ac1: a full run on C1, then a partial run on C2 with only opens.
    let w = world().await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    w.upload("c2c2c2c", 2, false, junit(&[("opens", "pass")]))
        .await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "green"
            && s["tests"][0]["commit"] == "c2c2c2c"
            && s["tests"][1]["commit"] == "c1c1c1c"
            && s["commit"] == "c1c1c1c",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_full_run_absence_beats_a_partial_pass_on_the_same_commit() -> Result<()> {
    // no-mixed-subjects-ac2: opens is listed; the full run of C omits it, a partial run of C passes it.
    let w = world().await?;
    w.list(&[A]).await?;
    w.upload("c1c1c1c", 1, true, junit(&[("refuses", "pass")]))
        .await?;
    w.upload("c1c1c1c", 1, false, junit(&[("opens", "pass")]))
        .await?;
    let s = w.status().await?;
    let x = s["tests"]
        .as_array()
        .and_then(|tests| tests.iter().find(|t| t["address"] == A));
    ensure!(
        x.is_some_and(|x| x["status"] == "missing" && x["commit"] == "c1c1c1c")
            && s["status"] == "red",
        "{s}"
    );
    Ok(())
}

fn stamp(commit: &str, order: i64, dirty: bool) -> Value {
    json!({"commit":commit,"order":order,"branch":"master","default_branch":true,"dirty":dirty,"full":true})
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn every_report_records_its_commit_and_uploader() -> Result<()> {
    // no-manual-pass-ac1: bot's own token uploads; the stored report names the commit and bot.
    let w = world().await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(
        code == 200 && created["token"].as_str().is_some_and(|t| !t.is_empty()),
        "{code} {created}"
    );
    let bot = created["token"].as_str().unwrap();
    let (code, reply) = w
        .raw_upload(
            Some(bot),
            stamp("c1c1c1c", 1, false),
            junit(&[("opens", "pass")]),
        )
        .await?;
    ensure!(
        code == 200
            && reply["commit"] == "c1c1c1c"
            && reply["uploader"] == "bot"
            && reply["report"].is_i64(),
        "{reply}"
    );
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let row = pool
        .get()
        .await?
        .query_one(
            "SELECT commit_sha, uploader FROM joppa.test_reports WHERE id=$1",
            &[&reply["report"].as_i64().unwrap()],
        )
        .await?;
    let (commit, uploader): (String, String) = (row.get(0), row.get(1));
    ensure!(
        commit == "c1c1c1c" && uploader == "bot",
        "{commit} {uploader}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_upload_without_a_token_is_refused() -> Result<()> {
    // no-manual-pass-ac2
    let w = world().await?;
    let (code, reply) = w
        .raw_upload(
            None,
            stamp("c1c1c1c", 1, false),
            junit(&[("opens", "pass")]),
        )
        .await?;
    ensure!(
        code == 401 && reply["error"] == "token required",
        "{code} {reply}"
    );
    ensure!(
        w.status().await?["tests"][0]["status"] == "not_run",
        "nothing was stored"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_upload_with_an_invalid_token_is_refused() -> Result<()> {
    // no-manual-pass-ac3
    let w = world().await?;
    let (code, reply) = w
        .raw_upload(
            Some("bad"),
            stamp("c1c1c1c", 1, false),
            junit(&[("opens", "pass")]),
        )
        .await?;
    ensure!(
        code == 401 && reply["error"] == "token is not valid",
        "{code} {reply}"
    );
    ensure!(
        w.status().await?["tests"][0]["status"] == "not_run",
        "nothing was stored"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_criterion_save_cannot_set_a_test_result() -> Result<()> {
    // no-manual-pass-ac4: bot saves A with a verification or a passing list; X stays not_run.
    let w = world().await?;
    w.list(&[A]).await?;
    let x_status = |s: &Value| {
        s["tests"]
            .as_array()
            .and_then(|tests| tests.iter().find(|t| t["address"] == A))
            .map(|x| x["status"].clone())
    };
    let s = w.status().await?;
    ensure!(x_status(&s) == Some(json!("not_run")), "{s}");
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(
        code == 200 && created["token"].as_str().is_some_and(|t| !t.is_empty()),
        "{code} {created}"
    );
    let bot = created["token"].as_str().unwrap().to_owned();
    let save = |data: Value, criterion: Value| {
        let command = json!({"op":"save_object","kind":"ac","id":"key-rights","revision":criterion["current_revision"],"parent":criterion["document"]["parent"],"data":data,"links":criterion["document"]["links"],"reason":"Save the criterion"});
        post(
            &w,
            &bot,
            "/commands",
            json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}),
        )
    };
    // Control: a valid data change saves.
    let criterion = planning_criterion(&w).await?;
    let mut data = criterion["document"]["data"].clone();
    data["description"] =
        json!("Given: a key.\nWhen: bot reads.\nThen: bot can read.\nEvidence: the test report.");
    let (code, reply) = save(data, criterion).await?;
    ensure!(code == 200, "control save: {code} {reply}");
    let before = w.status().await?;
    // Fixture 1: data.verification="passed" is refused.
    let criterion = planning_criterion(&w).await?;
    let mut data = criterion["document"]["data"].clone();
    data["verification"] = json!("passed");
    let (code, reply) = save(data, criterion).await?;
    ensure!(
        code != 200 && reply["error"] == "verification comes from test reports",
        "fixture 1: {code} {reply}"
    );
    let after = w.status().await?;
    ensure!(
        after == before && x_status(&after) == Some(json!("not_run")),
        "fixture 1: {before} {after}"
    );
    // Fixture 2: an extra field that lists X as passing may save or be refused.
    let before = w.status().await?;
    let criterion = planning_criterion(&w).await?;
    let mut data = criterion["document"]["data"].clone();
    data["test_results"] = json!([{"address": A, "status": "pass"}]);
    let (code, reply) = save(data, criterion).await?;
    let after = w.status().await?;
    ensure!(
        after == before && x_status(&after) == Some(json!("not_run")),
        "fixture 2 ({code} {reply}): {before} {after}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_dirty_run_is_stored_but_does_not_count() -> Result<()> {
    // result-has-output-ac2: opens failed on C; a dirty run at C says it passes.
    let w = world().await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    let (code, reply) = w
        .raw_upload(
            Some(&w.token),
            stamp("c1c1c1c", 1, true),
            junit(&[("opens", "pass"), ("refuses", "pass")]),
        )
        .await?;
    ensure!(
        code == 200 && reply["replayed"] == false,
        "the dirty report is stored: {reply}"
    );
    let s = w.status().await?;
    ensure!(
        s["status"] == "red" && s["tests"][0]["status"] == "fail",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_text_change_makes_the_list_a_draft_until_it_is_saved() -> Result<()> {
    // check-list-frozen-ac1 and ac2: the text changes, the list waits; saving it ends the draft for good.
    let w = world().await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "green before the text change"
    );
    w.revise_text(1, "Only read and verify until launch")
        .await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "grey" && s["reason"] == "tests_draft" && s["tests_draft"] == true,
        "{s}"
    );
    w.list(&[A, B]).await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "green" && s["tests_draft"] == false && s["tests_revision"] == 2,
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_changed_test_list_makes_a_new_list_revision() -> Result<()> {
    // reduced-coverage-reviewed-ac1: the agent removes one test; the list keeps its own history.
    let w = world().await?;
    ensure!(w.status().await?["tests_revision"] == 1, "first list");
    w.list(&[A]).await?;
    let s = w.status().await?;
    ensure!(
        s["tests_revision"] == 2
            && s["tests"].as_array().map(Vec::len) == Some(1)
            && s["tests_saved_by"] == "agent",
        "{s}"
    );
    Ok(())
}

const STALE_LIST: &str = "stale test list revision; read the tests again";

/// Save the list of ac:key-rights over HTTP as the agent, with an optional list revision.
async fn save_list(w: &World, tests: &[&str], revision: Option<u32>) -> Result<(u16, Value)> {
    let mut command = json!({"op":"set_ac_tests","target":"ac:key-rights","tests":tests,"reason":"Link the tests"});
    if let Some(revision) = revision {
        command["revision"] = json!(revision);
    }
    post(
        w,
        &w.token,
        "/commands",
        json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}),
    )
    .await
}

/// The listed test addresses of one test status read.
fn listed(status: &Value) -> Vec<&str> {
    status["tests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["address"].as_str())
        .collect()
}

async fn journal_count(pool: &deadpool_postgres::Pool, workspace: &str) -> Result<i64> {
    Ok(pool
        .get()
        .await?
        .query_one(
            "SELECT count(*) FROM joppa.events WHERE workspace=$1",
            &[&workspace],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_stale_test_list_save_is_refused_and_a_current_or_absent_revision_saves() -> Result<()> {
    // checks-listed-first-ac4: criterion C has [A] at tests_revision 2.
    let w = world().await?;
    w.list(&[A]).await?;
    let s = w.status().await?;
    ensure!(listed(&s) == [A] && s["tests_revision"] == 2, "{s}");
    let (code, reply) = save_list(&w, &[A, B], Some(1)).await?;
    ensure!(
        code == 409 && reply["error"] == STALE_LIST,
        "{code} {reply}"
    );
    let s = w.status().await?;
    ensure!(listed(&s) == [A] && s["tests_revision"] == 2, "{s}");
    let (code, reply) = save_list(&w, &[A, B], Some(2)).await?;
    ensure!(code == 200, "{code} {reply}");
    let s = w.status().await?;
    ensure!(listed(&s) == [A, B] && s["tests_revision"] == 3, "{s}");
    let (code, reply) = save_list(&w, &[B], None).await?;
    ensure!(code == 200, "{code} {reply}");
    let s = w.status().await?;
    ensure!(listed(&s) == [B] && s["tests_revision"] == 4, "{s}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn concurrent_first_test_list_saves_with_revision_zero_accept_exactly_one() -> Result<()> {
    // checks-listed-first-ac4: criterion D has no saved list; agents P and Q save at the same time.
    let w = world().await?;
    let p = planning(&w).await?;
    let root = w.base.split("/api/").next().unwrap();
    let read = || async {
        w.http
            .get(format!(
                "{root}/api/workspaces/{}/tests?ac=criterion:a",
                p.ws
            ))
            .bearer_auth(&w.token)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await
    };
    let s = read().await?;
    ensure!(
        listed(&s).is_empty() && s["tests_revision"].is_null(),
        "{s}"
    );
    let save = |actor: &'static str, tests: Value| {
        p.write(
            actor,
            json!({"op":"set_ac_tests","target":"criterion:a","tests":tests,"revision":0,"reason":"Link the tests"}),
        )
    };
    let (first, second) = tokio::join!(save("agent", json!([A])), save("other", json!([B])));
    let (winner, refusal) = match (first, second) {
        (Ok(_), Err(e)) => (A, e),
        (Err(e), Ok(_)) => (B, e),
        (first, second) => anyhow::bail!("expected one success: {first:?} / {second:?}"),
    };
    ensure!(format!("{refusal:#}") == STALE_LIST, "{refusal:#}");
    let s = read().await?;
    ensure!(listed(&s) == [winner] && s["tests_revision"] == 1, "{s}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_batch_with_a_stale_test_list_save_changes_nothing() -> Result<()> {
    // checks-listed-first-ac4: a valid command and a stale list save in one batch.
    let w = world().await?;
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let workspace = w.base.rsplit('/').next().unwrap();
    let objects = || async {
        w.http
            .get(format!("{}/objects?parent=*&limit=100", w.base))
            .bearer_auth(&w.token)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await
    };
    let objects_before = objects().await?;
    let status_before = w.status().await?;
    let count_before = journal_count(&pool, workspace).await?;
    ensure!(status_before["tests_revision"] == 1, "{status_before}");
    let reply = planning_batch(&w, json!([
        {"command":{"op":"save_object","kind":"note","id":"batch-note","revision":0,"parent":null,"data":{"title":"batch-note","owner":"owner","body":"Signal"},"links":[],"reason":"Save"}},
        {"command":{"op":"set_ac_tests","target":"ac:key-rights","tests":[A],"revision":0,"reason":"Stale list"}}
    ])).await?;
    ensure!(
        reply["result"]["isError"] == true
            && reply["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|error| error.ends_with(STALE_LIST)),
        "{reply}"
    );
    let objects_after = objects().await?;
    ensure!(
        objects_after == objects_before,
        "{objects_before} / {objects_after}"
    );
    ensure!(
        !objects_after.to_string().contains("batch-note"),
        "{objects_after}"
    );
    let status_after = w.status().await?;
    ensure!(
        status_after == status_before,
        "{status_before} / {status_after}"
    );
    ensure!(journal_count(&pool, workspace).await? == count_before);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_journal_of_test_list_saves_without_revision_replays_to_the_same_lists() -> Result<()> {
    // checks-listed-first-ac4: the journal holds list saves with no revision field.
    let w = world().await?;
    w.list(&[A]).await?;
    w.list(&[B, A]).await?;
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let workspace = w.base.rsplit('/').next().unwrap();
    let conn = pool.get().await?;
    let saves: i64 = conn
        .query_one(
            "SELECT count(*) FROM joppa.events WHERE workspace=$1 AND command->>'op'='set_ac_tests' AND NOT command ? 'revision'",
            &[&workspace],
        )
        .await?
        .get(0);
    ensure!(saves == 3, "{saves} list saves without revision");
    let before = w.status().await?;
    ensure!(
        listed(&before) == [B, A] && before["tests_revision"] == 3,
        "{before}"
    );
    // Remove the stored lists and the board, then rebuild the workspace from its journal.
    for statement in [
        "DELETE FROM joppa.state_entries WHERE workspace=$1 AND section='ac_tests'",
        "DELETE FROM joppa.board_cache WHERE workspace=$1",
    ] {
        conn.execute(statement, &[&workspace]).await?;
    }
    store::backfill_board(&pool).await?;
    let after = w.status().await?;
    ensure!(
        listed(&after) == listed(&before) && after["tests_revision"] == before["tests_revision"],
        "{before} / {after}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_dirty_failure_on_a_newer_commit_does_not_turn_green_red() -> Result<()> {
    // result-has-output-ac2, the other way: a clean pass, then a dirty failure on a newer commit.
    let w = world().await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let (code, _) = w
        .raw_upload(
            Some(&w.token),
            stamp("c2c2c2c", 2, true),
            junit(&[("opens", "fail"), ("refuses", "pass")]),
        )
        .await?;
    ensure!(code == 200, "the dirty report is stored");
    let s = w.status().await?;
    ensure!(s["status"] == "green" && s["commit"] == "c1c1c1c", "{s}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_shared_upload_fixture_is_accepted() -> Result<()> {
    // The hook test sends bodies of exactly this shape to its fake Joppa.
    let w = world().await?;
    let fixture: Value = serde_json::from_str(include_str!("fixtures/report-upload.json"))?;
    let (code, reply) = w
        .raw_upload(
            Some(&w.token),
            fixture["stamp"].clone(),
            fixture["junit"].as_str().unwrap().into(),
        )
        .await?;
    ensure!(code == 200 && reply["tests"] == 2, "{code} {reply}");
    ensure!(
        w.status().await?["status"] == "green",
        "the fixture proves both tests"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_upload_cannot_name_its_uploader() -> Result<()> {
    // no-manual-pass-ac1: the uploader comes from the token. A body that claims one is refused.
    let w = world().await?;
    let mut claimed = stamp("c1c1c1c", 1, false);
    claimed["uploader"] = json!("owner");
    let (code, reply) = w
        .raw_upload(Some(&w.token), claimed, junit(&[("opens", "pass")]))
        .await?;
    ensure!(code == 422, "{code} {reply}");
    ensure!(
        w.status().await?["tests"][0]["status"] == "not_run",
        "nothing was stored"
    );
    let (_, reply) = w
        .raw_upload(
            Some(&w.token),
            stamp("c1c1c1c", 1, false),
            junit(&[("opens", "pass")]),
        )
        .await?;
    ensure!(
        reply["uploader"] == "agent",
        "the token names the uploader: {reply}"
    );
    Ok(())
}

async fn upload_xml(
    w: &World,
    commit: &str,
    order: i64,
    full: bool,
    default_branch: bool,
    xml: &str,
) -> Result<()> {
    let mut stamp = stamp(commit, order, false);
    stamp["full"] = json!(full);
    stamp["default_branch"] = json!(default_branch);
    let (code, reply) = w.raw_upload(Some(&w.token), stamp, xml.into()).await?;
    ensure!(code == 200, "{code} {reply}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_red_test_stays_red_on_a_partial_run_that_skips_it() -> Result<()> {
    // defect-stays-red-ac1: opens fails on C1; a partial report for C2 has only refuses.
    let w = world().await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    w.upload("c2c2c2c", 2, false, junit(&[("refuses", "pass")]))
        .await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["status"] == "fail" && s["tests"][0]["commit"] == "c1c1c1c",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_red_test_clears_when_a_newer_commit_passes_it() -> Result<()> {
    // defect-stays-red-ac2
    let w = world().await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    w.upload(
        "c2c2c2c",
        2,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["status"] == "pass"
            && s["tests"][0]["commit"] == "c2c2c2c"
            && s["status"] == "green",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_skipped_listed_test_keeps_the_criterion_grey() -> Result<()> {
    // missing-is-not-pass-ac1: X passes on C1; the later full report for C2 has X skipped.
    let w = world().await?;
    w.list(&[A]).await?;
    let s = w.status().await?;
    ensure!(
        s["tests_draft"] == false && s["tests"].as_array().map(Vec::len) == Some(1),
        "{s}"
    );
    upload_xml(&w, "c1c1c1c", 1, true, true, r#"<testsuites><testsuite name="tests/auth/login.test.ts"><testsuite name="login"><testcase name="opens" /></testsuite></testsuite></testsuites>"#).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == A
            && s["tests"][0]["status"] == "pass"
            && s["status"] == "green",
        "{s}"
    );
    upload_xml(&w, "c2c2c2c", 2, true, true, r#"<testsuites><testsuite name="tests/auth/login.test.ts"><testsuite name="login"><testcase name="opens"><skipped/></testcase></testsuite></testsuite></testsuites>"#).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == A
            && s["tests"][0]["status"] == "skipped"
            && s["status"] == "grey",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_todo_listed_test_keeps_the_criterion_grey() -> Result<()> {
    // missing-is-not-pass-ac3: X passes on C1; the later full report for C2 has X as todo.
    let w = world().await?;
    let (code, reply) = post(
        &w,
        &w.owner,
        "/commands",
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"confirm","req":"key","revision":1}}),
    )
    .await?;
    ensure!(code == 200, "{code} {reply}");
    w.list(&[A]).await?;
    let s = w.status().await?;
    ensure!(
        s["tests_draft"] == false && s["tests"].as_array().map(Vec::len) == Some(1),
        "{s}"
    );
    upload_xml(&w, "c1c1c1c", 1, true, true, r#"<testsuites><testsuite name="tests/auth/login.test.ts"><testsuite name="login"><testcase name="opens" /></testsuite></testsuite></testsuites>"#).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == A
            && s["tests"][0]["status"] == "pass"
            && s["status"] == "green",
        "{s}"
    );
    upload_xml(&w, "c2c2c2c", 2, true, true, r#"<testsuites><testsuite name="tests/auth/login.test.ts"><testsuite name="login"><testcase name="opens"><skipped message="TODO"/></testcase></testsuite></testsuite></testsuites>"#).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == A && s["tests"][0]["status"] == "todo" && s["status"] == "grey",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_report_from_a_feature_branch_does_not_change_status() -> Result<()> {
    // result-names-subject-ac1: the criterion is grey; a passing report arrives from branch "feat".
    let w = world().await?;
    upload_xml(
        &w,
        "f1f1f1f",
        5,
        true,
        false,
        &junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "grey" && s["tests"][0]["status"] == "not_run",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_saves_a_test_list_by_address() -> Result<()> {
    // checks-listed-first-ac1: skopanev (the owner token) saves one address.
    let w = world().await?;
    // A fresh criterion, so it starts with no saved test list.
    let command = json!({"op":"revise","req":"key","expected_revision":1,"capability":"restrict","title":"Key rights","body":"Scope","owner":"owner","consumers":["customer"],"reason":"One more criterion","acs":[{"id":"key-rights","text":"Only read and verify before launch"},{"id":"key-audit","text":"Every key change is in the audit log"}]});
    let reply = w
        .http
        .post(format!("{}/commands", w.base))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
        .send()
        .await?;
    ensure!(
        reply.status().is_success(),
        "revise: {}",
        reply.text().await?
    );
    let read = || async {
        let s: Value = w
            .http
            .get(format!("{}/tests?ac=ac:key-audit", w.base))
            .bearer_auth(&w.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        anyhow::Ok(s)
    };
    let s = read().await?;
    ensure!(
        s["tests"] == json!([]) && s["tests_saved_by"].is_null(),
        "{s}"
    );
    let command = json!({"op":"set_ac_tests","target":"ac:key-audit","tests":["a.test.ts > login > ok"],"reason":"Link the test"});
    let reply = w
        .http
        .post(format!("{}/commands", w.base))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
        .send()
        .await?;
    ensure!(reply.status().is_success(), "{}", reply.text().await?);
    let s = read().await?;
    ensure!(
        s["tests"]
            .as_array()
            .map(|t| t.iter().map(|t| t["address"].clone()).collect::<Vec<_>>())
            == Some(vec![json!("a.test.ts > login > ok")])
            && s["tests_saved_by"] == "owner",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_cannot_save_a_test_address_without_a_file() -> Result<()> {
    // checks-listed-first-ac2: skopanev (the owner token) saves "login > ok" with no file.
    let w = world().await?;
    let before = w.status().await?;
    let command = json!({"op":"set_ac_tests","target":"ac:key-rights","tests":["login > ok"],"reason":"Link the test"});
    let (code, reply) = post(
        &w,
        &w.owner,
        "/commands",
        json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}),
    )
    .await?;
    ensure!(
        code == 409 && reply["error"] == "a test address needs file > name at least",
        "{code} {reply}"
    );
    let after = w.status().await?;
    ensure!(
        after["tests"] == before["tests"] && after["tests_saved_by"] == before["tests_saved_by"],
        "{before} {after}"
    );
    Ok(())
}

const FAILING_REFUSES: &str = r#"<testsuites><testsuite name="tests/auth/login.test.ts"><testsuite name="login"><testcase name="opens" /><testcase name="refuses"><failure message="expected 403, got 200" /></testcase></testsuite></testsuite></testsuites>"#;

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_test_shows_its_status_commit_and_message() -> Result<()> {
    // old-results-history-ac1: agent bot reads A over HTTP with its own token.
    let w = world().await?;
    upload_xml(&w, "c1c1c1c", 1, true, true, FAILING_REFUSES).await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let reply = w
        .http
        .get(format!("{}/tests?ac=ac:key-rights", w.base))
        .bearer_auth(bot)
        .send()
        .await?;
    let code = reply.status();
    let text = reply.text().await?;
    ensure!(code.is_success(), "{code} {text}");
    let s: Value = serde_json::from_str(&text)?;
    ensure!(
        s["tests"][0]["status"] == "pass"
            && s["tests"][0]["commit"] == "c1c1c1c"
            && s["tests"][1]["status"] == "fail"
            && s["tests"][1]["commit"] == "c1c1c1c"
            && s["tests"][1]["message"] == "expected 403, got 200",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn mcp_returns_each_test_with_status_and_commit() -> Result<()> {
    // old-results-history-ac3: agent bot calls the MCP tool joppa_tests with its own token.
    let w = world().await?;
    upload_xml(&w, "c1c1c1c", 1, true, true, FAILING_REFUSES).await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let workspace = w.base.rsplit('/').next().unwrap();
    let reply = mcp(
        &w,
        bot,
        "joppa_tests",
        json!({"workspace":workspace,"ac":"ac:key-rights"}),
    )
    .await?;
    let s = read_result("joppa_tests", &reply)?;
    ensure!(
        s["tests"][0]["status"] == "pass"
            && s["tests"][0]["commit"] == "c1c1c1c"
            && s["tests"][1]["status"] == "fail"
            && s["tests"][1]["commit"] == "c1c1c1c"
            && s["tests"][1]["message"] == "expected 403, got 200",
        "{reply}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn commits_behind_counts_newer_default_branch_commits() -> Result<()> {
    // time-not-stale-ac1: A is decided on C; five newer commits ran other tests only.
    let w = world().await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    for order in 2..=6 {
        upload_xml(&w, &format!("{order}{order}{order}{order}{order}{order}{order}"), order, false, true,
            r#"<testsuites><testsuite name="tests/other.test.ts"><testcase name="other" /></testsuite></testsuites>"#).await?;
    }
    let s = w.status().await?;
    ensure!(
        s["commit"] == "c1c1c1c" && s["commits_behind"] == 5 && s["status"] == "green",
        "{s}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_test_tag_in_code_links_nothing() -> Result<()> {
    // binding-pins-contract-ac1: "[ac:key-rights] other" fails in the same full report.
    let w = world().await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(w.status().await?["status"] == "green", "green before");
    w.upload(
        "c2c2c2c",
        2,
        true,
        junit(&[
            ("opens", "pass"),
            ("refuses", "pass"),
            ("[ac:key-rights] other", "fail"),
        ]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "a tag in a test name links nothing"
    );
    Ok(())
}

async fn task(w: &World) -> Result<Value> {
    Ok(w.http
        .get(format!("{}/tests?task=task:key/t1", w.base))
        .bearer_auth(&w.token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_green_criterion_does_not_mark_an_open_task_done() -> Result<()> {
    // all-bound-checks-pass-ac1: an agent bot reads open task T under green criterion A.
    let w = world().await?;
    ensure!(
        task(&w).await?["done"] == false,
        "not done before any report"
    );
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let workspace = w.base.rsplit('/').next().unwrap();
    let t = mcp_read(
        &w,
        bot,
        "joppa_tests",
        json!({"workspace":workspace,"task":"task:key/t1"}),
    )
    .await?;
    ensure!(
        t["done"] == false
            && t["verification"] == "passed"
            && t["criterion"]["address"] == "ac:key-rights"
            && t["criterion"]["status"] == "green"
            && t["criterion"]["commit"] == "c1c1c1c",
        "{t}"
    );
    let object = mcp_read(
        &w,
        bot,
        "joppa_objects",
        json!({"workspace":workspace,"address":"task:key/t1"}),
    )
    .await?;
    ensure!(object["process"]["status"] == "open", "{object}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn only_complete_task_marks_a_task_done() -> Result<()> {
    let w = world().await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let workspace = w.base.rsplit('/').next().unwrap();
    for (token, command) in [
        (
            &w.owner,
            json!({"op":"save_object","kind":"task","id":"finished","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the key rights","owner":"owner","contract":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Check completion evidence"}),
        ),
        (
            &w.token,
            json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}}),
        ),
        (
            &w.token,
            json!({"op":"claim_task","id":"task:finished","revision":1}),
        ),
        (
            &w.token,
            json!({"op":"complete_task","id":"task:finished","revision":2,"evidence":"Implemented the contract"}),
        ),
    ] {
        let (code, reply) = post(
            &w,
            token,
            "/commands",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
        )
        .await?;
        ensure!(code == 200, "{command}: {code} {reply}");
    }
    let read = |address: &'static str| {
        mcp_read(
            &w,
            bot,
            "joppa_tests",
            json!({"workspace":workspace,"task":address}),
        )
    };
    for (commit, order, verification, cases) in [
        (None, 0, "unverified", vec![]),
        (
            Some("c1c1c1c"),
            1,
            "failed",
            vec![("opens", "fail"), ("refuses", "pass")],
        ),
        (
            Some("c2c2c2c"),
            2,
            "passed",
            vec![("opens", "pass"), ("refuses", "pass")],
        ),
    ] {
        if let Some(commit) = commit {
            w.upload(commit, order, true, junit(&cases)).await?;
        }
        let finished = read("task:finished").await?;
        ensure!(
            finished["done"] == true && finished["verification"] == verification,
            "{finished}"
        );
        let open = read("task:key/t1").await?;
        ensure!(
            open["done"] == false && open["verification"] == verification,
            "{open}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_joppa_tests_tool_states_that_only_complete_task_sets_done() -> Result<()> {
    let w = world().await?;
    let root = w.base.split("/api/").next().unwrap();
    let reply: Value = w
        .http
        .post(format!("{root}/mcp"))
        .bearer_auth(&w.token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await?
        .json()
        .await?;
    let description = reply["result"]["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|t| t["name"] == "joppa_tests"))
        .and_then(|t| t["description"].as_str())
        .ok_or_else(|| anyhow::anyhow!("no joppa_tests: {reply}"))?;
    ensure!(
        description.contains("Only complete_task sets done")
            && description.contains("verification follows the criterion status")
            && !description.contains("while its criterion is green"),
        "{description}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_is_no_longer_done_when_its_criterion_turns_red() -> Result<()> {
    // all-bound-checks-pass-ac2: a newer commit makes A red.
    let w = world().await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(task(&w).await?["done"] == false, "not completed on c1");
    w.upload(
        "c2c2c2c",
        2,
        true,
        junit(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    let t = task(&w).await?;
    ensure!(
        t["done"] == false
            && t["verification"] == "failed"
            && t["criterion"]["status"] == "red"
            && t["criterion"]["commit"] == "c2c2c2c",
        "{t}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_completed_task_stays_done_when_its_criterion_turns_red() -> Result<()> {
    // all-bound-checks-pass-ac2: complete_task finished T; a newer commit makes A red.
    let w = world().await?;
    for (token, command) in [
        (
            &w.owner,
            json!({"op":"save_object","kind":"task","id":"finished","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the key rights","owner":"owner","contract":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Check completion evidence"}),
        ),
        (
            &w.token,
            json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}}),
        ),
        (
            &w.token,
            json!({"op":"claim_task","id":"task:finished","revision":1}),
        ),
        (
            &w.token,
            json!({"op":"complete_task","id":"task:finished","revision":2,"evidence":"Implemented the contract"}),
        ),
    ] {
        let (code, reply) = post(
            &w,
            token,
            "/commands",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
        )
        .await?;
        ensure!(code == 200, "{command}: {code} {reply}");
    }
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let workspace = w.base.rsplit('/').next().unwrap();
    for (commit, order, verification, status, cases) in [
        (
            "c1c1c1c",
            1,
            "passed",
            "green",
            [("opens", "pass"), ("refuses", "pass")],
        ),
        (
            "c2c2c2c",
            2,
            "failed",
            "red",
            [("opens", "fail"), ("refuses", "pass")],
        ),
    ] {
        w.upload(commit, order, true, junit(&cases)).await?;
        let response = w
            .http
            .get(format!("{}/tests?task=task:finished", w.base))
            .bearer_auth(bot)
            .send()
            .await?;
        let code = response.status().as_u16();
        let http: Value = response.json().await?;
        ensure!(code == 200, "HTTP {commit}: {code} {http}");
        let mcp = mcp_read(
            &w,
            bot,
            "joppa_tests",
            json!({"workspace":workspace,"task":"task:finished"}),
        )
        .await?;
        for (via, t) in [("HTTP", &http), ("MCP", &mcp)] {
            ensure!(
                t["done"] == true
                    && t["verification"] == verification
                    && t["criterion"]["address"] == "ac:key-rights"
                    && t["criterion"]["status"] == status
                    && t["criterion"]["commit"] == commit,
                "{via} {commit}: {t}"
            );
        }
    }
    let object = mcp_read(
        &w,
        bot,
        "joppa_objects",
        json!({"workspace":workspace,"address":"task:finished"}),
    )
    .await?;
    ensure!(object["process"]["status"] == "done", "{object}");
    Ok(())
}

async fn post(w: &World, token: &str, path: &str, body: Value) -> Result<(u16, Value)> {
    let reply = w
        .http
        .post(format!("{}{path}", w.base))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;
    Ok((
        reply.status().as_u16(),
        reply.json().await.unwrap_or(Value::Null),
    ))
}
async fn save_note(w: &World, token: &str, id: &str) -> Result<(u16, Value)> {
    let command = json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"owner","body":"Signal"},"links":[],"reason":"Save"});
    post(
        w,
        token,
        "/commands",
        json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}),
    )
    .await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_token_writes_as_the_agent_and_stops_when_revoked() -> Result<()> {
    // agent-own-identity-ac1, write-records-agent-ac1, removal-at-once-ac2
    let w = world().await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(
        code == 200
            && created["token"]
                .as_str()
                .is_some_and(|t| t.starts_with("jpt_")),
        "{code} {created}"
    );
    let bot = created["token"].as_str().unwrap().to_owned();
    let (code, saved) = save_note(&w, &bot, "from-bot").await?;
    ensure!(code == 200, "{code} {saved}");
    let read: Value = w
        .http
        .get(format!("{}/objects?address=note:from-bot", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .json()
        .await?;
    ensure!(
        read["document"]["actor"] == "bot",
        "the write records the agent: {read}"
    );
    let id = created["id"].as_i64().unwrap();
    let (code, _) = post(
        &w,
        &w.owner,
        &format!("/agent-tokens/{id}/revoke"),
        json!({}),
    )
    .await?;
    ensure!(code == 200, "revoke");
    let (code, reply) = save_note(&w, &bot, "after-revoke").await?;
    ensure!(
        code == 401 && reply["error"] == "token revoked",
        "{code} {reply}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn only_the_owner_creates_an_agent_token() -> Result<()> {
    // removal-at-once-ac3 shape: a member who is not the owner cannot manage tokens.
    let w = world().await?;
    let (code, reply) = post(&w, &w.token, "/agent-tokens", json!({"agent":"bot2"})).await?;
    ensure!(
        code == 409 && reply["error"] == "workspace owner required",
        "{code} {reply}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_token_list_shows_the_access_of_each_token() -> Result<()> {
    // read-only-agent-token-ac3: the list shows read for planner-reader and write for bot.
    let w = world().await?;
    let (code, reader) = post(
        &w,
        &w.owner,
        "/agent-tokens",
        json!({"agent":"planner-reader","access":"read"}),
    )
    .await?;
    ensure!(
        code == 200
            && reader["access"] == "read"
            && reader["token"]
                .as_str()
                .is_some_and(|t| t.starts_with("jpt_")),
        "{code} {reader}"
    );
    let (code, bot) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200 && bot["access"] == "write", "{code} {bot}");
    let list = |token: &str| {
        w.http
            .get(format!("{}/agent-tokens", w.base))
            .bearer_auth(token)
            .send()
    };
    let reply = list(&w.owner).await?;
    ensure!(reply.status().as_u16() == 200, "{}", reply.status());
    let listed: Value = reply.json().await?;
    // No token or token hash among the fields.
    let fields = [
        "access",
        "agent",
        "created_at",
        "created_by",
        "id",
        "revoked",
    ];
    for (created, access) in [(&reader, "read"), (&bot, "write")] {
        let agent = &created["agent"];
        let item = listed["items"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|i| &i["agent"] == agent)
            .ok_or_else(|| anyhow::anyhow!("{agent} not listed: {listed}"))?;
        let mut keys: Vec<&str> = item
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, _)| k.as_str())
            .collect();
        keys.sort_unstable();
        ensure!(
            item["access"] == access
                && item["id"] == created["id"]
                && item["created_by"] == "owner"
                && item["created_at"].is_string()
                && item["revoked"] == false
                && keys == fields,
            "{item}"
        );
    }
    let reply = list(&w.token).await?;
    let code = reply.status().as_u16();
    let refused: Value = reply.json().await?;
    ensure!(
        code == 409 && refused["error"] == "workspace owner required",
        "{code} {refused}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_token_does_not_reach_another_workspace() -> Result<()> {
    // agent-needs-grant-ac3 shape: the token makes the agent a member of one workspace only.
    let a = world().await?;
    let b = world().await?;
    let (_, created) = post(&a, &a.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    let bot = created["token"].as_str().unwrap().to_owned();
    let reply = b
        .http
        .get(format!("{}/objects?limit=5&kind=domain", b.base))
        .bearer_auth(&bot)
        .send()
        .await?;
    ensure!(reply.status().as_u16() == 404, "{}", reply.status());
    Ok(())
}

/// Call one MCP tool and return the raw JSON-RPC reply. Transport and decoding errors fail here.
async fn mcp(w: &World, token: &str, tool: &str, arguments: Value) -> Result<Value> {
    let root = w.base.split("/api/").next().unwrap();
    Ok(w
        .http
        .post(format!("{root}/mcp"))
        .bearer_auth(token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}}))
        .send()
        .await?
        .json()
        .await?)
}

/// A JSON-RPC result without isError=true gives its decoded text content.
fn read_result(tool: &str, reply: &Value) -> Result<Value> {
    let result = &reply["result"];
    ensure!(result.is_object(), "{tool}: no result: {reply}");
    let text = result["content"][0]["text"].as_str();
    ensure!(
        result["isError"] != true,
        "{tool}: {}",
        text.unwrap_or_default()
    );
    let text = text.ok_or_else(|| anyhow::anyhow!("{tool}: no text: {reply}"))?;
    Ok(serde_json::from_str(text)?)
}

async fn mcp_read(w: &World, token: &str, tool: &str, arguments: Value) -> Result<Value> {
    read_result(tool, &mcp(w, token, tool, arguments).await?)
}

/// Every object address of joppa_objects with parent="*", one item per page.
async fn all_objects(w: &World, token: &str) -> Result<(Vec<String>, usize)> {
    let workspace = w.base.rsplit('/').next().unwrap();
    let (mut addresses, mut pages, mut cursor) = (vec![], 0, None::<String>);
    loop {
        let mut arguments = json!({"workspace":workspace,"parent":"*","limit":1});
        if let Some(cursor) = &cursor {
            arguments["cursor"] = json!(cursor);
        }
        let page = mcp_read(w, token, "joppa_objects", arguments).await?;
        pages += 1;
        for item in page["items"].as_array().into_iter().flatten() {
            let address = item["address"].as_str();
            addresses.push(
                address
                    .ok_or_else(|| anyhow::anyhow!("no address: {item}"))?
                    .to_owned(),
            );
        }
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => return Ok((addresses, pages)),
        }
    }
}

/// The four reads of read-only-agent-token-ac1 with valid arguments; joppa_objects reads its first page.
fn four_reads(w: &World) -> [(&'static str, Value); 4] {
    let workspace = w.base.rsplit('/').next().unwrap();
    [
        (
            "joppa_objects",
            json!({"workspace":workspace,"parent":"*","limit":1}),
        ),
        (
            "joppa_tests",
            json!({"workspace":workspace,"ac":"ac:key-rights"}),
        ),
        ("joppa_tasks", json!({"workspace":workspace})),
        ("joppa_tree", json!({"workspace":workspace})),
    ]
}

async fn read_token(w: &World) -> Result<String> {
    let (code, created) = post(
        w,
        &w.owner,
        "/agent-tokens",
        json!({"agent":"planner-reader","access":"read"}),
    )
    .await?;
    ensure!(
        code == 200 && created["access"] == "read" && created["agent"] == "planner-reader",
        "{code} {created}"
    );
    Ok(created["token"].as_str().unwrap_or_default().to_owned())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_token_with_read_access_reads_the_workspace_as_the_owner() -> Result<()> {
    // read-only-agent-token-ac1: owner S issues a read token for planner-reader in W.
    let w = world().await?;
    let reader = read_token(&w).await?;
    // all_objects checks every joppa_objects page of the reader.
    let (read, pages) = all_objects(&w, &reader).await?;
    let (owned, _) = all_objects(&w, &w.owner).await?;
    ensure!(pages > 1, "joppa_objects needs more than one page: {pages}");
    ensure!(
        ["requirement:key", "ac:key-rights", "task:key/t1"]
            .iter()
            .all(|a| read.iter().any(|r| r == a)),
        "the fixture objects are listed: {read:?}"
    );
    ensure!(read == owned, "reader {read:?} owner {owned:?}");
    for (tool, arguments) in four_reads(&w).into_iter().skip(1) {
        mcp_read(&w, &reader, tool, arguments).await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_read_check_fails_for_a_read_token_without_membership() -> Result<()> {
    // read-only-agent-token-ac1 control: planner-reader is a member of B only, so reads in A fail.
    let a = world().await?;
    let b = world().await?;
    let outsider = read_token(&b).await?;
    for (tool, arguments) in four_reads(&a) {
        let reply = mcp(&a, &outsider, tool, arguments).await?;
        let result = &reply["result"];
        ensure!(
            result.is_object()
                && result["isError"] == true
                && result["content"][0]["text"] == "Workspace not found",
            "{tool} without membership: {reply}"
        );
        ensure!(
            read_result(tool, &reply).is_err(),
            "the read check accepts {tool}: {reply}"
        );
    }
    Ok(())
}

const NEEDS: &str = "acceptance needs a full run on the commit where every listed test passes";

impl World {
    /// Accept requirement:key at its current revision; returns the status code and the reply text.
    async fn accept(&self, token: &str, commit: Option<&str>) -> Result<(u16, String)> {
        let read: Value = self
            .http
            .get(format!("{}/objects?address=requirement:key", self.base))
            .bearer_auth(&self.owner)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mut command = json!({"op":"accept_commit","target":"requirement:key","revision":read["document"]["revision"]});
        if let Some(commit) = commit {
            command["commit"] = json!(commit);
        }
        let reply = self
            .http
            .post(format!("{}/commands", self.base))
            .bearer_auth(token)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
            .send()
            .await?;
        Ok((reply.status().as_u16(), reply.text().await?))
    }
    async fn acceptance(&self) -> Result<Value> {
        let read: Value = self
            .http
            .get(format!("{}/objects?address=requirement:key", self.base))
            .bearer_auth(&self.owner)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(read["process"]["acceptance"].clone())
    }
}

async fn refused_acceptance(w: &World, commit: &str, error: &str) -> Result<()> {
    let (code, reply) = w.accept(&w.owner, Some(commit)).await?;
    ensure!(code != 200 && reply.contains(error), "{code} {reply}");
    ensure!(w.acceptance().await?.is_null(), "nothing is accepted");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_acceptance_on_a_green_full_run_names_its_commit_and_actor() -> Result<()> {
    // acceptance-names-subject-ac1, acceptance-shows-actor-ac1
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let (code, reply) = w.accept(&w.owner, Some("c1c1c1c")).await?;
    ensure!(code == 200, "{code} {reply}");
    let acceptance = w.acceptance().await?;
    ensure!(
        acceptance["commit"] == "c1c1c1c" && acceptance["actor"] == "owner",
        "{acceptance}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_failing_test_on_the_full_run_refuses_acceptance() -> Result<()> {
    // accept-needs-all-passed-ac1
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_revision_without_a_new_run_refuses_acceptance() -> Result<()> {
    // accept-needs-all-passed-ac2, new-revision-new-acceptance-ac1
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let (code, reply) = w.accept(&w.owner, Some("c1c1c1c")).await?;
    ensure!(code == 200, "{code} {reply}");
    ensure!(
        w.acceptance().await?["commit"] == "c1c1c1c",
        "revision 1 is accepted"
    );
    w.revise_text(1, "Only read and verify, before launch and after")
        .await?;
    ensure!(
        w.acceptance().await?.is_null(),
        "revision 2 is not accepted by the old acceptance"
    );
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn passes_only_in_partial_runs_refuse_acceptance() -> Result<()> {
    // accept-needs-all-passed-ac3
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.upload(
        "c1c1c1c",
        1,
        false,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_acceptance_without_a_commit_is_refused() -> Result<()> {
    // acceptance-names-subject-ac2
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let (code, reply) = w.accept(&w.owner, None).await?;
    ensure!(
        code != 200 && reply.contains("acceptance needs a commit"),
        "{code} {reply}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_skipped_test_on_the_full_run_refuses_acceptance() -> Result<()> {
    // acceptance-names-subject-ac3
    let w = world().await?;
    w.list(&[A, B]).await?;
    upload_xml(&w, "c1c1c1c", 1, true, true, r#"<testsuites><testsuite name="tests/auth/login.test.ts"><testsuite name="login"><testcase name="opens" /><testcase name="refuses"><skipped/></testcase></testsuite></testsuite></testsuites>"#).await?;
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_commit_only_on_a_feature_branch_refuses_acceptance() -> Result<()> {
    // acceptance-names-subject-ac4
    let w = world().await?;
    w.list(&[A, B]).await?;
    upload_xml(
        &w,
        "f1f1f1f",
        1,
        true,
        false,
        &junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    refused_acceptance(&w, "f1f1f1f", "acceptance needs a default-branch commit").await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_cannot_accept_a_requirement() -> Result<()> {
    // decisions-from-people-ac1
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let (code, reply) = w.accept(&w.token, Some("c1c1c1c")).await?;
    ensure!(
        code != 200 && reply.contains("only a person records this decision"),
        "{code} {reply}"
    );
    ensure!(w.acceptance().await?.is_null(), "nothing is accepted");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_criterion_without_tests_refuses_acceptance() -> Result<()> {
    // accept-needs-all-passed-ac1: an untested criterion does not pass by being empty.
    let w = world().await?;
    let command = json!({"op":"revise","req":"key","expected_revision":1,"capability":"restrict","title":"Key rights","body":"Scope","owner":"owner","consumers":["customer"],"reason":"One more criterion","acs":[{"id":"key-rights","text":"Only read and verify before launch"},{"id":"key-audit","text":"Every key change is in the audit log"}]});
    let reply = w
        .http
        .post(format!("{}/commands", w.base))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
        .send()
        .await?;
    ensure!(
        reply.status().is_success(),
        "revise: {}",
        reply.text().await?
    );
    w.upload(
        "c1c1c1c",
        1,
        true,
        junit(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_listed_test_missing_from_the_full_run_refuses_acceptance() -> Result<()> {
    // accept-needs-all-passed-ac1: a listed test that did not run is not a pass.
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.upload("c1c1c1c", 1, true, junit(&[("opens", "pass")]))
        .await?;
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_task_roles_read_their_criterion_results_over_http_and_mcp() -> Result<()> {
    async fn command(w: &World, value: Value) -> Result<()> {
        let (code, reply) = post(
            w,
            &w.owner,
            "/commands",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":value}),
        )
        .await?;
        ensure!(code == 200, "{value}: {code} {reply}");
        Ok(())
    }
    async fn read(w: &World, bot: &str, address: &str) -> Result<Value> {
        let response = w
            .http
            .get(format!("{}/tests?task={address}", w.base))
            .bearer_auth(bot)
            .send()
            .await?;
        let code = response.status().as_u16();
        let http: Value = response.json().await?;
        ensure!(code == 200, "{address}: {code} {http}");
        let root = w.base.split("/api/").next().unwrap();
        let workspace = w.base.rsplit('/').next().unwrap();
        let response: Value = w.http.post(format!("{root}/mcp")).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_tests","arguments":{"workspace":workspace,"task":address}}})).send().await?.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        let mcp: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        ensure!(http == mcp, "HTTP {http}; MCP {mcp}");
        Ok(http)
    }
    let w = world().await?;
    let schema: Value = w
        .http
        .get(format!("{}/tree", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut types = schema["schema"]["types"].clone();
    types["task"]["parents"] = json!(["*"]);
    types["job"] = types["task"].clone();
    types["job"]["parents"] = json!(["*"]);
    types["check"] = types["ac"].clone();
    types["check"]["parents"] = json!(["*"]);
    types["record"] = types["project"].clone();
    types["record"]["parents"] = json!(["*"]);
    command(&w,json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check configured task and criterion names"})).await?;
    command(&w,json!({"op":"save_object","kind":"check","id":"configured","revision":0,"parent":"requirement:key","data":{"title":"The configured result passes","description":"Check the configured contract","owner":"owner","depends_on":[],"before_launch":false,"decision_required":false},"reason":"Create a configured criterion"})).await?;
    command(&w,json!({"op":"set_ac_tests","target":"check:configured","tests":[A,B],"reason":"Bind the configured criterion"})).await?;
    for (kind, id, parent) in [
        ("job", "native", "ac:key-rights"),
        ("task", "configured", "check:configured"),
        ("job", "configured", "check:configured"),
    ] {
        command(&w,json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":parent,"data":{"title":"Implement the configured result","owner":"owner","contract":"Implement the agreed behavior and provide evidence","purpose":"implementation","projects":[]},"reason":"Check task result reads"})).await?;
    }
    command(&w,json!({"op":"save_object","kind":"record","id":"not-work","revision":0,"parent":"check:configured","data":{"title":"A document has no task role","owner":"owner"},"reason":"Check the role boundary"})).await?;
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap();
    let tasks = [
        ("task:key/t1", "ac:key-rights"),
        ("job:native", "ac:key-rights"),
        ("task:configured", "check:configured"),
        ("job:configured", "check:configured"),
    ];
    for (commit, order, expected, verification, cases) in [
        (None, 0, "grey", "unverified", vec![]),
        (
            Some("c1c1c1c"),
            1,
            "green",
            "passed",
            vec![("opens", "pass"), ("refuses", "pass")],
        ),
        (
            Some("c2c2c2c"),
            2,
            "red",
            "failed",
            vec![("opens", "fail"), ("refuses", "pass")],
        ),
    ] {
        if let Some(commit) = commit {
            w.upload(commit, order, true, junit(&cases)).await?;
        }
        for (address, criterion) in tasks {
            let status = read(&w, bot, address).await?;
            ensure!(
                status["address"] == address
                    && status["verification"] == verification
                    && status["done"] == false
                    && status["criterion"]["address"] == criterion
                    && status["criterion"]["status"] == expected
                    && status["criterion"]["commit"] == json!(commit),
                "{status}"
            );
        }
    }
    let response = w
        .http
        .get(format!("{}/tests?task=record:not-work", w.base))
        .bearer_auth(bot)
        .send()
        .await?;
    ensure!(response.status().as_u16() == 409, "{}", response.status());
    let refused: Value = response.json().await?;
    ensure!(refused["error"] == "unknown task", "{refused}");
    Ok(())
}

const BLOCKED: &str = "Task is blocked for planning";
const NO_MATCH: &str = "no available Task matches this claim";

// failed-task-blocks-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_blocked_task_refuses_claims_and_leaves_the_queue_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    let position = p.blocked("work:t").await?;
    ensure!(p.saved().await?["position"] == position);
    let block = json!({"actor":"other","reason":"Verification failed","evidence":"test x failed on abc123","position":position});
    let t = p.read("work:t").await?;
    ensure!(
        t["current_revision"] == 5
            && t["process"]["status"] == "open"
            && t["process"]["assignee"].is_null()
            && t["process"]["blocked"] == block,
        "{t}"
    );
    ensure!(p.saved_object("work:t").await?["process"]["blocked"] == block);

    p.refuse_command(
        "claim_task on blocked T",
        json!({"op":"claim_task","id":"work:t","revision":5}),
        BLOCKED,
    )
    .await?;
    p.refuse_command(
        "claim_next_task with only blocked T",
        json!({"op":"claim_next_task"}),
        BLOCKED,
    )
    .await?;

    let root = w.base.split("/api/").next().unwrap();
    for view in ["available", "all"] {
        let http: Value = w
            .http
            .get(format!("{root}/api/workspaces/{}/tasks?view={view}", p.ws))
            .bearer_auth(&w.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mcp = p.mcp("joppa_tasks", json!({"view":view})).await?;
        let mcp: Value = serde_json::from_str(
            mcp["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_default(),
        )?;
        for (transport, page) in [("HTTP", http), ("MCP", mcp)] {
            let tasks = page["tasks"].as_array().cloned().unwrap_or_default();
            let t = tasks.iter().find(|t| t["address"] == "work:t");
            if view == "available" {
                ensure!(t.is_none(), "{transport} available lists T: {page}");
            } else {
                ensure!(
                    t.is_some_and(|t| t["claimable"] == false && t["waiting_reason"] == BLOCKED),
                    "{transport} all: {page}"
                );
            }
        }
    }

    // The HTTP envelope keeps its checks; the raw command meets the shared validation.
    let before = p.saved().await?;
    let valid = json!({"op":"save_object","kind":"spec","id":"envelope","revision":0,"parent":null,"data":{"title":"Envelope","body":"Check the envelope","owner":"owner"},"reason":"Check the envelope"});
    let id = || json!(uuid::Uuid::new_v4().to_string());
    let mut with_actor = valid.clone();
    with_actor["actor"] = json!("other");
    let mut unknown_field = valid.clone();
    unknown_field["extra"] = json!(true);
    let mut missing_field = valid.clone();
    missing_field.as_object_mut().unwrap().remove("reason");
    for (case, body) in [
        ("missing request_id", json!({"command":valid})),
        ("missing command", json!({"request_id":id()})),
        (
            "unknown envelope field",
            json!({"request_id":id(),"command":valid,"extra":true}),
        ),
        (
            "envelope actor",
            json!({"request_id":id(),"command":valid,"actor":"other"}),
        ),
        (
            "command actor",
            json!({"request_id":id(),"command":with_actor}),
        ),
        (
            "unknown command field",
            json!({"request_id":id(),"command":unknown_field}),
        ),
        (
            "missing command field",
            json!({"request_id":id(),"command":missing_field}),
        ),
        (
            "unknown operation",
            json!({"request_id":id(),"command":{"op":"made_up_task_operation","id":"work:t","revision":5}}),
        ),
        (
            "command is not an object",
            json!({"request_id":id(),"command":"claim_next_task"}),
        ),
    ] {
        let (code, reply) = p.http(body).await?;
        ensure!((400..500).contains(&code), "{case}: {code} {reply}");
        ensure!(
            p.saved().await? == before,
            "{case}: the refusal changed saved state"
        );
    }
    let (code, reply) = p.http(json!({"request_id":id(),"command":valid})).await?;
    ensure!(code == 200, "the valid envelope: {code} {reply}");
    store::check_journal(&p.pool).await?;
    Ok(())
}

// failed-task-blocks-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn claim_next_task_takes_the_ordinary_task_beside_a_blocked_one() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        let position = p.blocked("work:t").await?;
        p.save("work:ordinary", Some("criterion:a"), json!({"title":"Ordinary","owner":"owner","contract":"Implement this criterion","purpose":"implementation"}), json!([])).await?;
        let reply = p
            .send(mcp, &json!({"op":"claim_next_task"}))
            .await?
            .map_err(anyhow::Error::msg)?;
        let ordinary = p.read("work:ordinary").await?;
        ensure!(
            ordinary["process"]["assignee"] == "agent",
            "mcp={mcp}: {reply} {ordinary}"
        );
        let t = p.read("work:t").await?;
        ensure!(
            t["current_revision"] == 5
                && t["process"]["assignee"].is_null()
                && t["process"]["blocked"]["position"] == position,
            "mcp={mcp}: {t}"
        );
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// failed-task-blocks-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn claim_next_task_keeps_the_no_match_error_for_held_and_unpublished_tasks() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    let revision = p.revision("work:t0").await?;
    p.write(
        "other",
        json!({"op":"claim_task","id":"work:t0","revision":revision}),
    )
    .await?;
    p.refuse_command(
        "only T0, held by Q",
        json!({"op":"claim_next_task"}),
        NO_MATCH,
    )
    .await?;

    let p = planning(&w).await?;
    p.ready().await?;
    p.claim("other", "criterion:a").await?;
    p.write("other", json!({"op":"save_object","kind":"work","id":"new","revision":0,"parent":"criterion:a","data":{"title":"New","owner":"owner","contract":"Implement this criterion","purpose":"implementation"},"reason":"Plan the work"})).await?;
    p.refuse_command(
        "only an unpublished Task",
        json!({"op":"claim_next_task"}),
        NO_MATCH,
    )
    .await?;
    store::check_journal(&p.pool).await?;
    Ok(())
}

const NEEDS_CONTRACT: &str = "blocked Task needs a new contract or retirement";
const OWNER_REQUIRED: &str = "active Task owner required";

impl Planning<'_> {
    /// Fixture 1 of failed-task-blocks-ac3: L blocks T under ready A, then P claims A.
    /// Returns the block of T.
    async fn blocked_plan(&self) -> Result<Value> {
        let position = self.blocked("work:t").await?;
        self.claim("agent", "criterion:a").await?;
        Ok(
            json!({"actor":"other","reason":"Verification failed","evidence":"test x failed on abc123","position":position}),
        )
    }
    /// P saves `key` under A with `contract` and `links`.
    async fn plan_task(&self, key: &str, contract: &str, links: Value) -> Result<()> {
        let (kind, id) = key.split_once(':').unwrap();
        let revision = self.revision(key).await.unwrap_or(0);
        self.write("agent", json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":"criterion:a","data":{"title":"T","owner":"owner","contract":contract,"purpose":"implementation"},"links":links,"reason":"Plan the work"})).await?;
        Ok(())
    }
    /// P finishes A through HTTP or MCP.
    async fn finish(&self, mcp: bool) -> Result<()> {
        self.send(
            mcp,
            &json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
        Ok(())
    }
}

fn finish_command() -> Value {
    json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"})
}

// failed-task-blocks-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn finish_planning_needs_a_new_contract_for_a_blocked_task_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        let block = p.blocked_plan().await?;
        // Fixture 3 and its controls: an identical save and an unrelated new Task.
        p.refuse_command("unchanged blocked T", finish_command(), NEEDS_CONTRACT)
            .await?;
        p.plan_task("work:t", "Implement this criterion", json!([]))
            .await?;
        p.refuse_command("identical save of T", finish_command(), NEEDS_CONTRACT)
            .await?;
        p.plan_task("work:new", "Implement other work", json!([]))
            .await?;
        p.refuse_command("only an unrelated Task", finish_command(), NEEDS_CONTRACT)
            .await?;
        let a = p.read("criterion:a").await?;
        let t = p.read("work:t").await?;
        ensure!(
            a["process"]["planning_claim"]["actor"] == "agent" && t["process"]["blocked"] == block,
            "{a} {t}"
        );

        // Fixture 1: P amends the T contract text and finishes.
        let revision = p.revision("work:t").await?;
        p.write("agent", json!({"op":"amend_task","id":"work:t","revision":revision,"title":"T","contract":"Implement the criterion with a new fix","projects":[],"reason":"Amend the contract"})).await?;
        p.finish(mcp).await?;
        let a = p.read("criterion:a").await?;
        let t = p.read("work:t").await?;
        ensure!(
            a["process"]["planning_claim"].is_null() && t["process"]["blocked"].is_null(),
            "mcp={mcp}: {a} {t}"
        );
        ensure!(p.saved_object("work:t").await?["process"]["blocked"].is_null());
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// failed-task-blocks-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn finish_planning_clears_a_block_after_a_new_dependency_and_names_it() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.blocked_plan().await?;
    // Fixture 2: P adds Task C under A and a blocking link from T to C.
    p.plan_task("work:c", "Deliver the prerequisite", json!([]))
        .await?;
    p.plan_task(
        "work:t",
        "Implement this criterion",
        json!([{"target":"work:c","relation":"depends_on","revision":1,"blocking":true}]),
    )
    .await?;
    p.finish(false).await?;
    let a = p.read("criterion:a").await?;
    let t = p.read("work:t").await?;
    ensure!(
        a["process"]["planning_claim"].is_null() && t["process"]["blocked"].is_null(),
        "{a} {t}"
    );
    let root = w.base.split("/api/").next().unwrap();
    let http: Value = w
        .http
        .get(format!("{root}/api/workspaces/{}/tasks?view=all", p.ws))
        .bearer_auth(&w.token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mcp = p.mcp("joppa_tasks", json!({"view":"all"})).await?;
    let mcp: Value = serde_json::from_str(
        mcp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default(),
    )?;
    for (transport, page) in [("HTTP", http), ("MCP", mcp)] {
        let tasks = page["tasks"].as_array().cloned().unwrap_or_default();
        let t = tasks.iter().find(|t| t["address"] == "work:t");
        ensure!(
            t.is_some_and(|t| t["claimable"] == false
                && t["waiting_reason"]
                    .as_str()
                    .is_some_and(|r| r.contains("work:c") && r != BLOCKED)),
            "{transport} all: {page}"
        );
    }
    store::check_journal(&p.pool).await?;
    Ok(())
}

// failed-task-blocks-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_planner_retires_a_blocked_task_and_other_actors_cannot_over_http_and_mcp() -> Result<()>
{
    let w = world().await?;
    let p = planning(&w).await?;
    let position = p.blocked("work:t").await?;
    p.save("work:u", Some("criterion:a"), json!({"title":"U","owner":"owner","contract":"Implement this criterion","purpose":"implementation"}), json!([])).await?;
    let retire = |key: &str, revision: u64| json!({"op":"retire_task","id":key,"revision":revision,"reason":"The planned work replaces this Task"});
    // P is not yet the planner, the owner or the workspace owner.
    let revision = p.revision("work:t").await?;
    p.refuse_command(
        "P retires blocked T without a claim",
        retire("work:t", revision),
        OWNER_REQUIRED,
    )
    .await?;
    let before = p.saved().await?;
    ensure!(p.write("other", retire("work:t", revision)).await.is_err());
    ensure!(p.saved().await? == before, "Q changed saved state");
    p.claim("agent", "criterion:a").await?;
    let u = p.revision("work:u").await?;
    p.refuse_command("P retires unblocked U", retire("work:u", u), OWNER_REQUIRED)
        .await?;

    // Fixture 4: P retires blocked T and finishes.
    p.send(true, &retire("work:t", revision))
        .await?
        .map_err(anyhow::Error::msg)?;
    p.finish(false).await?;
    let a = p.read("criterion:a").await?;
    let t = p.read("work:t").await?;
    ensure!(
        a["process"]["planning_claim"].is_null() && t["process"]["status"] == "retired",
        "{a} {t}"
    );
    let block = store::read_objects(
        &p.pool,
        &p.ws,
        "owner",
        &store::ObjectScope {
            address: Some("work:t"),
            revision: Some(5),
            ..Default::default()
        },
    )
    .await?;
    ensure!(
        block["document"]["position"] == position
            && block["document"]["reason"] == "Verification failed",
        "{block}"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}

const UNBLOCK_REASON: &str = "Startup failure; checkout discarded";
const PLANNER_OR_OWNER: &str = "planning holder or workspace owner required";

impl Planning<'_> {
    /// Fixture 1 of failed-task-blocks-ac5: L blocks T under A, and U under B
    /// depends on T with a blocking link pinned to the current T revision.
    async fn blocked_dependency(&self) -> Result<()> {
        self.blocked("work:t").await?;
        let pin = self.revision("work:t").await?;
        self.save("work:u", Some("criterion:b"), json!({"title":"U","owner":"owner","contract":"Implement this criterion","purpose":"implementation"}),
            json!([{"target":"work:t","relation":"depends_on","revision":pin,"blocking":true}])).await
    }
    /// P sends unblock_task for T and finish_planning for A in one MCP batch.
    async fn unblock_and_finish(&self) -> Result<Value> {
        let revision = self.revision("work:t").await?;
        self.mcp(
            "joppa_batch",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"operations":[
                {"command":unblock_t(revision)},{"command":finish_command()}
            ]}),
        )
        .await
    }
}

fn unblock_t(revision: u64) -> Value {
    json!({"op":"unblock_task","id":"work:t","revision":revision,"reason":UNBLOCK_REASON,"evidence":"Run: r1"})
}

// failed-task-blocks-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn unblock_task_refuses_a_member_and_a_stale_revision_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    // Q holds the planning claim on A. The token member is M without a claim.
    let p = planning(&w).await?;
    p.blocked_dependency().await?;
    p.claim("other", "criterion:a").await?;
    let revision = p.revision("work:t").await?;
    p.refuse_command("M unblocks T", unblock_t(revision), PLANNER_OR_OWNER)
        .await?;
    // The token member is P with the planning claim on A.
    let p = planning(&w).await?;
    p.blocked_dependency().await?;
    p.claim("agent", "criterion:a").await?;
    let revision = p.revision("work:t").await?;
    p.refuse_command(
        "P unblocks T at a stale revision",
        unblock_t(revision - 1),
        "stale Task revision; read the Task again",
    )
    .await?;
    store::check_journal(&p.pool).await?;
    Ok(())
}

// failed-task-blocks-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_planner_unblocks_a_task_and_finishes_planning_in_one_batch() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.blocked_dependency().await?;
    p.claim("agent", "criterion:a").await?;
    let t0 = p.read("work:t").await?;
    let u0 = p.read("work:u").await?;
    let block = t0["process"]["blocked"].clone();
    let response = p.unblock_and_finish().await?;
    ensure!(response["result"]["isError"] != true, "{response}");

    let t = p.read("work:t").await?;
    let revision = t0["current_revision"].as_u64().unwrap() + 1;
    ensure!(
        t["process"]["blocked"].is_null()
            && t["current_revision"] == revision
            && t["address"] == t0["address"]
            && t["document"]["data"] == t0["document"]["data"]
            && t["document"]["parent"] == t0["document"]["parent"]
            && t["document"]["links"] == t0["document"]["links"]
            && t["process"]["origin_revision"] == t0["process"]["origin_revision"]
            && t["process"]["assignee"].is_null()
            && t["process"]["status"] == "open",
        "T changed beyond its block: {t0} {t}"
    );
    let u = p.read("work:u").await?;
    ensure!(
        u["review_required"] == false
            && u["process"]["review_required"] == false
            && u["document"]["links"] == u0["document"]["links"],
        "U needs review: {u}"
    );
    let a = p.read("criterion:a").await?;
    ensure!(a["process"]["planning_claim"].is_null(), "{a}");

    // T is claimable through HTTP and MCP.
    let root = w.base.split("/api/").next().unwrap();
    let http: Value = w
        .http
        .get(format!("{root}/api/workspaces/{}/tasks?view=all", p.ws))
        .bearer_auth(&w.token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mcp = p.mcp("joppa_tasks", json!({"view":"all"})).await?;
    let mcp: Value = serde_json::from_str(
        mcp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default(),
    )?;
    for (transport, page) in [("HTTP", http), ("MCP", mcp)] {
        let tasks = page["tasks"].as_array().cloned().unwrap_or_default();
        ensure!(
            tasks
                .iter()
                .any(|t| t["address"] == "work:t" && t["claimable"] == true),
            "{transport} all: {page}"
        );
    }

    // The T history keeps the block and records the unblock with P, reason and evidence.
    let read = async |revision: u64| {
        store::read_objects(
            &p.pool,
            &p.ws,
            "owner",
            &store::ObjectScope {
                address: Some("work:t"),
                revision: Some(revision as u32),
                ..Default::default()
            },
        )
        .await
    };
    let blocked = read(revision - 1).await?;
    let unblocked = read(revision).await?;
    ensure!(
        blocked["document"]["position"] == block["position"]
            && blocked["document"]["reason"] == "Verification failed"
            && unblocked["document"]["actor"] == "agent"
            && unblocked["document"]["reason"] == UNBLOCK_REASON,
        "{blocked} {unblocked}"
    );
    let history = store::read(&p.pool, &p.ws, "owner", &store::Scope::full()).await?;
    let event = history["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["position"] == unblocked["document"]["position"])
        .cloned()
        .unwrap_or_default();
    ensure!(
        event["actor"] == "agent"
            && event["command"]["op"] == "unblock_task"
            && event["command"]["reason"] == UNBLOCK_REASON
            && event["command"]["evidence"] == "Run: r1",
        "{event}"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}

// failed-task-blocks-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_refused_finish_rolls_back_the_unblock_in_the_same_batch() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.blocked_dependency().await?;
    // Fixture 2: a second unchanged blocked Task V under A.
    p.save("work:v", Some("criterion:a"), json!({"title":"V","owner":"owner","contract":"Implement this criterion","purpose":"implementation"}), json!([])).await?;
    let revision = p.revision("work:v").await?;
    p.write(
        "other",
        json!({"op":"claim_task","id":"work:v","revision":revision}),
    )
    .await?;
    p.write("other", json!({"op":"block_task","id":"work:v","revision":revision + 1,"reason":"Verification failed","evidence":"test y failed on abc123"})).await?;
    p.claim("agent", "criterion:a").await?;
    let before = p.saved().await?;
    let response = p.unblock_and_finish().await?;
    ensure!(
        response["result"]["isError"] == true
            && response["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|e| e.contains(NEEDS_CONTRACT)),
        "{response}"
    );
    ensure!(
        p.saved().await? == before,
        "the refused batch changed saved state"
    );
    let a = p.read("criterion:a").await?;
    let t = p.read("work:t").await?;
    ensure!(
        a["process"]["planning_claim"]["actor"] == "agent" && !t["process"]["blocked"].is_null(),
        "{a} {t}"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}

// failed-task-blocks-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_workspace_owner_unblocks_without_a_claim_and_the_journal_replays() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.blocked_dependency().await?;
    let revision = p.revision("work:t").await?;
    p.write("owner", unblock_t(revision)).await?;
    let t = p.read("work:t").await?;
    ensure!(
        t["process"]["blocked"].is_null() && t["current_revision"] == revision + 1,
        "{t}"
    );
    let before = p.saved().await?;
    let refused = p.write("owner", unblock_t(revision + 1)).await;
    ensure!(
        refused
            .as_ref()
            .is_err_and(|e| e.to_string() == "Task is not blocked"),
        "{refused:?}"
    );
    ensure!(
        p.saved().await? == before,
        "the refusal changed saved state"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}
