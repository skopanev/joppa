//! Test results over HTTP: a real server, a member token and records of test outcomes.
//! Each test proves one Evidence criterion through the endpoints, not the status function.
use anyhow::{Context, Result, ensure};
use joppa::{commands::Command as Operation, sessions, store};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

#[path = "common/record_http.rs"]
mod record_http;

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

/// Outcomes of tests/auth/login.test.ts > login, as joppa_record_results takes them.
fn outcomes(cases: &[(&str, &str)]) -> Value {
    record_http::cases("tests/auth/login.test.ts > login", cases)
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

// The planner's unstarted list leaves out a criterion that is green by its test reports.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_green_criterion_leaves_the_unstarted_list() -> Result<()> {
    let w = world().await?;
    ready_for_planning(&w).await?;
    // The fixture Task t1 is open work, so retire it: unstarted lists criteria without active work.
    let retire = json!({"op":"retire_task","req":"key","revision":1,"id":"t1","reason":"Fixture work no longer applies"});
    let (code, reply) = post(
        &w,
        &w.owner,
        "/commands",
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":retire}),
    )
    .await?;
    ensure!(code == 200, "{code} {reply}");
    let unstarted = || async {
        let reply: Value = w
            .http
            .get(format!("{}/objects?work_filter=unstarted", w.base))
            .bearer_auth(&w.token)
            .send()
            .await?
            .json()
            .await?;
        Ok::<_, anyhow::Error>(
            reply["items"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|item| item["address"] == "ac:key-rights"),
        )
    };
    ensure!(unstarted().await?, "a grey ready criterion is unstarted");
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "the record must make A green"
    );
    ensure!(!unstarted().await?, "a green criterion is not unstarted");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn planning_claim_refuses_a_recorded_green_over_http_and_mcp_without_mutation() -> Result<()>
{
    let w = world().await?;
    ready_for_planning(&w).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "the record must make A green"
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
        p.write(
            "owner",
            json!({"op":"enroll_service","id":actor,"role":"agent"}),
        )
        .await?;
    }
    let schema = store::tree_schema(&p.pool, &p.ws, "owner", None).await?;
    p.write("owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
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
        self.http_as(&self.w.token, body).await
    }
    /// The `token` member posts the request body to the HTTP command endpoint.
    async fn http_as(&self, token: &str, body: Value) -> Result<(u16, Value)> {
        let root = self.w.base.split("/api/").next().unwrap();
        let reply = self
            .w
            .http
            .post(format!("{root}/api/workspaces/{}/commands", self.ws))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;
        let code = reply.status().as_u16();
        Ok((code, reply.json().await.unwrap_or(Value::Null)))
    }
    /// P calls one MCP tool in this workspace.
    async fn mcp(&self, tool: &str, arguments: Value) -> Result<Value> {
        self.mcp_as(&self.w.token, tool, arguments).await
    }
    /// The `token` member calls one MCP tool in this workspace.
    async fn mcp_as(&self, token: &str, tool: &str, mut arguments: Value) -> Result<Value> {
        let root = self.w.base.split("/api/").next().unwrap();
        arguments["workspace"] = json!(self.ws);
        Ok(self.w.http.post(format!("{root}/mcp")).bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}}))
            .send().await?.json().await?)
    }
    /// P sends `command` through HTTP or MCP and gets the result or the error text.
    async fn send(&self, mcp: bool, command: &Value) -> Result<std::result::Result<Value, String>> {
        self.send_as(&self.w.token, mcp, command).await
    }
    /// The `token` member sends `command` through HTTP or MCP.
    async fn send_as(
        &self,
        token: &str,
        mcp: bool,
        command: &Value,
    ) -> Result<std::result::Result<Value, String>> {
        let request_id = uuid::Uuid::new_v4().to_string();
        if mcp {
            let response = self
                .mcp_as(
                    token,
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
            .http_as(token, json!({"request_id":request_id,"command":command}))
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
        self.refuse_command_as(&self.w.token, case, command, expected)
            .await
    }
    /// The `token` member sends the command through HTTP and then MCP, with the same checks.
    async fn refuse_command_as(
        &self,
        token: &str,
        case: &str,
        command: Value,
        expected: &str,
    ) -> Result<()> {
        let before = self.saved().await?;
        let (code, reply) = self
            .http_as(
                token,
                json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
            )
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
            .mcp_as(
                token,
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

// Planning claim guards with agent members. The people tests of planning-claim-ac1 are
// in reports_http_parts/planning_people.rs.
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
    // A planner may hold claims in several REQs.
    p.claim("agent", "criterion:c").await?;
    ensure!(p.read("criterion:c").await?["process"]["planning_claim"]["actor"] == "agent");
    ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "agent");

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

    // A blocks on C at C's current revision; C is assessed ready but not green.
    let p = planning(&w).await?;
    let c = p.prerequisite("Prerequisite scope").await?;
    p.depend(c, true).await?;
    p.ready().await?;
    let a = p.read("criterion:a").await?;
    ensure!(
        a["process"]["assessment"]["state"] == "ready"
            && a["review_required"] == false
            && a["work_ready"] == false
            && a["process"]["readiness_blocker"]["target"] == "criterion:c"
            && p.revision("criterion:c").await? == c,
        "{a}"
    );
    p.refuse(
        "dependency-blocked A",
        "criterion:a",
        a["current_revision"].as_u64().unwrap(),
        &joppa::dependencies::reason("criterion:c"),
    )
    .await?;

    // dependencies-before-priority: A's non-blocking dependency names an older revision of
    // C. An ordering link of a criterion pins nothing, so it needs no review.
    let p = planning(&w).await?;
    let c = p.prerequisite("Prerequisite scope").await?;
    p.depend(c, false).await?;
    ensure!(p.prerequisite("Sharper prerequisite scope").await? > c);
    p.ready().await?;
    let a = p.read("criterion:a").await?;
    ensure!(
        a["process"]["assessment"]["state"] == "ready"
            && a["review_required"] == false
            && a["work_ready"] == true,
        "{a}"
    );
    p.claim("agent", "criterion:a").await?;
    ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "agent");

    let p = planning(&w).await?;
    p.ready().await?;
    p.t0().await?;
    p.write(
        "agent",
        json!({"op":"set_ac_tests","target":"criterion:a","tests":[A,B],"reason":"Link the tests"}),
    )
    .await?;
    let root = w.base.split("/api/").next().unwrap();
    let base = format!("{root}/api/workspaces/{}", p.ws);
    let passed = outcomes(&[("opens", "pass"), ("refuses", "pass")]);
    record_http::recorded(&w.http, root, &p.ws, &w.token, "c1c1c1c", passed).await?;
    let status: Value = w
        .http
        .get(format!("{base}/tests?ac=criterion:a"))
        .bearer_auth(&w.token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(
        status["status"] == "green",
        "the record must make A green: {status}"
    );
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

include!("reports_http_parts/planning_people.rs");
include!("reports_http_parts/planning_people_wait.rs");

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn planning_claim_uses_the_current_batch_test_list_in_both_directions() -> Result<()> {
    let w = world().await?;
    w.list(&[A]).await?;
    ready_for_planning(&w).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "fail")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "the stored list must be green"
    );
    // check-list-frozen-ac3: each list save resets the criterion, so the batch assesses it again.
    let assess = json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"Known work scope"}});
    let before = planning_criterion(&w).await?;
    // The list save in the batch gives the criterion its next revision.
    let claim = json!({"op":"claim_planning","ac":"ac:key-rights","revision":before["current_revision"].as_u64().unwrap_or(0) + 1});
    let reply = planning_batch(&w, json!([
        {"command":{"op":"set_ac_tests","target":"ac:key-rights","tests":[A,B],"reason":"Include the failing check"}},
        {"command":assess},
        {"command":claim}
    ])).await?;
    ensure!(
        reply["result"]["isError"] != true && reply.get("error").is_none(),
        "current grey list must permit planning: {reply}"
    );
    ensure!(
        w.status().await?["status"] == "grey",
        "the new list must be grey"
    );
    ensure!(
        planning_criterion(&w).await?["process"]["planning_claim"]["actor"] == "agent",
        "claim missing"
    );
    let (code, reply) = post(&w, &w.token, "/commands", json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{
        "op":"release_planning","ac":"ac:key-rights","reason":"Check the opposite batch transition"}})).await?;
    ensure!(code == 200, "{code} {reply}");
    let before = planning_criterion(&w).await?;
    // The list save in the batch gives the criterion its next revision.
    let claim = json!({"op":"claim_planning","ac":"ac:key-rights","revision":before["current_revision"].as_u64().unwrap_or(0) + 1});
    let reply = planning_batch(&w, json!([
        {"command":{"op":"set_ac_tests","target":"ac:key-rights","tests":[A],"reason":"Use the passing check"}},
        {"command":assess},
        {"command":claim}
    ])).await?;
    ensure!(
        reply["result"]["isError"] != true && reply.get("error").is_none(),
        "the earlier passing report must not refuse planning: {reply}"
    );
    ensure!(
        w.status().await?["status"] == "grey",
        "the earlier report must not count after the list save"
    );
    ensure!(
        planning_criterion(&w).await?["process"]["planning_claim"]["actor"] == "agent",
        "claim missing"
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
        let status = match role {
            "ac" => "tests",
            "group" | "requirement" => "children",
            _ => "none",
        };
        types[kind] = json!({"title":title,"role":role,"status":status,"root":parent.is_none(),"parents":parent.into_iter().collect::<Vec<_>>()});
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
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "fail")]),
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
    // check-list-frozen-ac3: the archive resets every criterion below bundle:root.
    ensure!(
        after["verification_summary"] == json!({"total":2,"passed":0,"failed":0}),
        "{after}"
    );
    Ok(())
}

// status-current-only-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_listed_test_without_a_result_is_not_run_and_grey() -> Result<()> {
    // A lists opens (X) and refuses (Y); the only record of C with X passing.
    let w = world().await?;
    w.record("c1c1c1c", outcomes(&[("opens", "pass")])).await?;
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
    let test = |address: &str| {
        s["tests"]
            .as_array()
            .and_then(|tests| tests.iter().find(|t| t["address"] == address))
    };
    ensure!(
        test(A).is_some_and(|x| x["status"] == "pass" && x["commit"] == "c1c1c1c")
            && test(B).is_some_and(|y| y["status"] == "not_run")
            && s["status"] == "grey"
            && s["reason"] == "not all tests passed",
        "{s}"
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
    // A save gives the document a revision, so only tests_revision may change.
    ensure!(
        after["status"] == before["status"]
            && after["tests"] == before["tests"]
            && x_status(&after) == Some(json!("not_run")),
        "fixture 2 ({code} {reply}): {before} {after}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_text_change_greys_the_criterion_until_a_newer_record() -> Result<()> {
    // check-list-frozen-ac1 and ac2: the text changes, and only a newer record counts.
    let w = world().await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "green before the text change"
    );
    w.revise_text(1, "Only read and verify until launch")
        .await?;
    let s = w.status().await?;
    // check-list-frozen-ac2: the report before the text change no longer counts.
    ensure!(
        s["status"] == "grey" && s["reason"] == "not all tests passed" && listed(&s) == [A, B],
        "{s}"
    );
    w.record(
        "c2c2c2c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(
        w.status().await?["status"] == "green",
        "a new record after the change is green"
    );
    Ok(())
}

/// The test list fields that a report must not change.
fn saved_list(status: &Value) -> Value {
    json!([
        addresses(status),
        status["tests_revision"],
        status["tests_saved_by"]
    ])
}

/// Report and result rows of this workspace.
async fn report_rows(w: &World) -> Result<(i64, i64)> {
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let workspace = w.base.rsplit('/').next().unwrap();
    let row = pool.get().await?.query_one("SELECT (SELECT count(*) FROM joppa.test_reports WHERE workspace=$1),(SELECT count(*) FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1)", &[&workspace]).await?;
    Ok((row.get(0), row.get(1)))
}

/// A clean full C1 report passes every test of A, then the text of A changes: A is grey.
async fn changed_after_c1(w: &World) -> Result<(Value, Value)> {
    let first = w.record("c1c1c1c", passing()).await?;
    ensure!(first["replayed"] == false, "{first}");
    ensure!(
        w.status().await?["status"] == "green",
        "green before the text change"
    );
    w.revise_text(1, "Only read and verify until launch")
        .await?;
    let s = w.status().await?;
    ensure!(s["status"] == "grey", "before the record: {s}");
    Ok((first, s))
}

fn passing() -> Value {
    outcomes(&[("opens", "pass"), ("refuses", "pass")])
}

// check-list-frozen-ac4, fixture 1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_identical_c1_report_after_a_text_change_turns_the_criterion_green() -> Result<()> {
    let w = world().await?;
    let (first, before) = changed_after_c1(&w).await?;
    let rows = report_rows(&w).await?;
    let again = w.record("c1c1c1c", passing()).await?;
    ensure!(
        again["record"] == first["record"] && again["replayed"] == true,
        "{again}"
    );
    ensure!(
        report_rows(&w).await? == rows,
        "the repeat copied report rows or results"
    );
    let s = w.status().await?;
    ensure!(
        s["status"] == "green" && saved_list(&s) == saved_list(&before),
        "after the record: {s}"
    );
    let workspace = w.base.rsplit('/').next().unwrap();
    let mcp = mcp_read(
        &w,
        &w.token,
        "joppa_tests",
        json!({"workspace":workspace,"ac":"ac:key-rights"}),
    )
    .await?;
    ensure!(mcp["status"] == "green", "MCP joppa_tests: {mcp}");
    Ok(())
}

// check-list-frozen-ac4, fixture 2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_c2_report_after_a_text_change_turns_the_criterion_green() -> Result<()> {
    let w = world().await?;
    let (_, before) = changed_after_c1(&w).await?;
    w.record("c2c2c2c", passing()).await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "green" && saved_list(&s) == saved_list(&before),
        "after the record: {s}"
    );
    Ok(())
}

// check-list-frozen-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn no_report_or_only_partial_reports_after_a_text_change_keep_the_criterion_grey()
-> Result<()> {
    let w = world().await?;
    let (_, before) = changed_after_c1(&w).await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "grey" && saved_list(&s) == saved_list(&before),
        "without a new report: {s}"
    );
    // A record that covers only one listed test keeps the criterion grey.
    w.record("c2c2c2c", outcomes(&[("opens", "pass")])).await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "grey" && saved_list(&s) == saved_list(&before),
        "after records: {s}"
    );
    Ok(())
}

// check-list-frozen-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_text_change_without_a_stored_boundary_counts_a_newer_record() -> Result<()> {
    let w = world().await?;
    let (_, before) = changed_after_c1(&w).await?;
    // A criterion whose text changed before boundaries were stored has none. The test
    // list is a field of its document, so no draft waits for another save.
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let workspace = w.base.rsplit('/').next().unwrap();
    let removed = pool
        .get()
        .await?
        .execute(
            "DELETE FROM joppa.state_entries WHERE workspace=$1 AND section='criterion_boundaries'",
            &[&workspace],
        )
        .await?;
    ensure!(removed == 1, "the text change stored no boundary");
    w.record("c2c2c2c", passing()).await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "green" && saved_list(&s) == saved_list(&before),
        "a text change without a boundary: {s}"
    );
    Ok(())
}

/// G holds R with criterion A and child group G2 with criterion A2. Sibling group H
/// holds criterion D. Each criterion lists A and B, one record passes both,
/// and each criterion is assessed ready.
async fn branches(w: &World) -> Result<Planning<'_>> {
    let p = Planning {
        w,
        pool: store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?,
        ws: format!("http-branches-{}", uuid::Uuid::new_v4().simple()),
    };
    store::execute_input(
        &p.pool,
        &p.ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Branch reset"}),
    )
    .await?;
    p.write(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(&p.pool, &p.ws, "owner", None).await?;
    p.write("owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure nested groups","types":{
        "area":{"title":"Area","role":"group","status":"children","root":true,"parents":["area"]},
        "spec":{"title":"Specification","role":"requirement","status":"children","parents":["area"]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]}
    }})).await?;
    for (key, parent) in [
        ("area:g", None),
        ("area:g2", Some("area:g")),
        ("area:h", None),
    ] {
        p.save(
            key,
            parent,
            json!({"title":key,"body":"Group scope","owner":"owner"}),
            json!([]),
        )
        .await?;
    }
    for (spec, area, criterion) in BRANCH_CRITERIA {
        p.save(
            spec,
            Some(area),
            json!({"title":spec,"body":"Implement the rule","owner":"owner"}),
            json!([]),
        )
        .await?;
        p.save(
            criterion,
            Some(spec),
            json!({"title":criterion,"owner":"owner"}),
            json!([]),
        )
        .await?;
        p.write(
            "agent",
            json!({"op":"set_ac_tests","target":criterion,"tests":[A,B],"reason":"Bind the tests"}),
        )
        .await?;
    }
    p.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    for (spec, _, criterion) in BRANCH_CRITERIA {
        p.confirm(
            spec,
            &[(criterion, json!({"ready":true,"reason":"Known scope"}))],
        )
        .await?;
    }
    Ok(p)
}

const BRANCH_CRITERIA: [(&str, &str, &str); 3] = [
    ("spec:r", "area:g", "criterion:a"),
    ("spec:r2", "area:g2", "criterion:a2"),
    ("spec:rh", "area:h", "criterion:d"),
];

impl Planning<'_> {
    /// Record `results` for `commit` in this workspace with the member token.
    async fn record(&self, commit: &str, results: Value) -> Result<()> {
        let root = self.w.base.split("/api/").next().unwrap();
        record_http::recorded(&self.w.http, root, &self.ws, &self.w.token, commit, results).await?;
        Ok(())
    }
    /// GET one path of this workspace over HTTP.
    async fn get(&self, path: String) -> Result<Value> {
        let root = self.w.base.split("/api/").next().unwrap();
        Ok(self
            .w
            .http
            .get(format!("{root}/api/workspaces/{}{path}", self.ws))
            .bearer_auth(&self.w.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    /// The owner saves `key` over HTTP with its current parent and no links.
    async fn owner_save(&self, key: &str, parent: Option<&str>, data: Value) -> Result<()> {
        let (kind, id) = key.split_once(':').unwrap();
        let root = self.w.base.split("/api/").next().unwrap();
        let command = json!({"op":"save_object","kind":kind,"id":id,"revision":self.revision(key).await?,"parent":parent,"data":data,"links":[],"reason":"Change the group"});
        self.w
            .http
            .post(format!("{root}/api/workspaces/{}/commands", self.ws))
            .bearer_auth(&self.w.owner)
            .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
    /// The parsed text of one MCP tool result.
    async fn mcp_read(&self, tool: &str, arguments: Value) -> Result<Value> {
        let reply = self.mcp(tool, arguments).await?;
        Ok(serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?)
    }
    async fn boundaries(&self) -> Result<Vec<(String, String)>> {
        Ok(self.pool.get().await?.query("SELECT key,value::text FROM joppa.state_entries WHERE workspace=$1 AND section='criterion_boundaries' ORDER BY key", &[&self.ws]).await?
            .iter().map(|r| (r.get(0), r.get(1))).collect())
    }
}

fn addresses(status: &Value) -> Vec<Value> {
    status["tests"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| t["address"].clone())
        .collect()
}

// check-list-frozen-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_group_change_resets_every_criterion_below_it_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = branches(&w).await?;
    let tests = |ac: &str| p.get(format!("/tests?ac={ac}"));
    let object = |address: &str| p.get(format!("/objects?address={address}"));
    let mut before = std::collections::BTreeMap::new();
    for (_, _, ac) in BRANCH_CRITERIA {
        let s = tests(ac).await?;
        ensure!(s["status"] == "green", "{ac} before the change: {s}");
        before.insert(ac, s);
    }
    let g = object("area:g").await?;
    ensure!(
        g["verification_summary"]["passed"].as_u64() > Some(0),
        "{g}"
    );
    let d_readiness = object("criterion:d").await?["process"]["assessment"].clone();
    let h_summary = object("area:h").await?["verification_summary"].clone();

    // Owner S changes only the text of G.
    p.owner_save(
        "area:g",
        None,
        json!({"title":"area:g","body":"Changed group scope","owner":"owner"}),
    )
    .await?;
    for ac in ["criterion:a", "criterion:a2"] {
        let s = tests(ac).await?;
        let old = &before[ac];
        ensure!(
            s["status"] == "grey"
                && addresses(&s) == addresses(old)
                && s["tests_revision"] == old["tests_revision"]
                && s["tests_saved_by"] == old["tests_saved_by"],
            "{ac} after the change: {s}"
        );
        let readiness = object(ac).await?["process"]["assessment"]["state"].clone();
        ensure!(readiness == "unassessed", "{ac} readiness: {readiness}");
    }
    let mcp = p
        .mcp_read("joppa_tests", json!({"ac":"criterion:a"}))
        .await?;
    ensure!(mcp["status"] == "grey", "MCP joppa_tests: {mcp}");
    let d = tests("criterion:d").await?;
    ensure!(d["status"] == "green", "{d}");
    ensure!(
        object("criterion:d").await?["process"]["assessment"] == d_readiness,
        "the readiness of D changed"
    );
    let g = p
        .mcp_read("joppa_objects", json!({"address":"area:g"}))
        .await?;
    ensure!(g["verification_summary"]["passed"] == 0, "{g}");
    ensure!(
        object("area:h").await?["verification_summary"] == h_summary,
        "the summary of H changed"
    );

    // Partial runs after the change combine: the latest result of each test decides.
    p.record("c2c2c2c", outcomes(&[("opens", "pass")])).await?;
    let s = tests("criterion:a").await?;
    ensure!(s["status"] == "grey", "after one record: {s}");
    p.record("c3c3c3c", outcomes(&[("refuses", "pass")]))
        .await?;
    let s = tests("criterion:a").await?;
    ensure!(s["status"] == "green", "after records: {s}");
    p.record(
        "c4c4c4c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let s = tests("criterion:a").await?;
    ensure!(s["status"] == "green", "after a record: {s}");
    ensure!(
        tests("criterion:d").await?["status"] == "green"
            && object("criterion:d").await?["process"]["assessment"] == d_readiness,
        "D changed"
    );

    // An identical revision of G stores no boundary; only a readiness write changes A.
    let revision = p.read("spec:r").await?["process"]["revision"].clone();
    p.write("owner", json!({"op":"assess","req":"spec:r","revision":revision,"ac":"criterion:a","assessment":{"ready":true,"reason":"Known scope"}})).await?;
    let boundaries = p.boundaries().await?;
    let readiness = object("criterion:a").await?["process"]["assessment"].clone();
    p.owner_save(
        "area:g",
        None,
        json!({"title":"area:g","body":"Changed group scope","owner":"owner"}),
    )
    .await?;
    ensure!(
        p.boundaries().await? == boundaries,
        "an identical revision stored a boundary"
    );
    ensure!(
        tests("criterion:a").await?["status"] == "green"
            && object("criterion:a").await?["process"]["assessment"] == readiness,
        "an identical revision changed A"
    );
    p.write("owner", json!({"op":"assess","req":"spec:r","revision":revision,"ac":"criterion:a","assessment":{"ready":false,"reason":"Which scope?","recipient":"owner"}})).await?;
    let a = object("criterion:a").await?;
    ensure!(
        a["process"]["assessment"]["state"] == "waiting"
            && tests("criterion:a").await?["status"] == "green"
            && p.boundaries().await? == boundaries,
        "{a}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_changed_test_list_makes_a_new_list_revision() -> Result<()> {
    // reduced-coverage-reviewed-ac1: the agent removes one test; the document of the
    // criterion gets a new revision.
    let w = world().await?;
    ensure!(w.status().await?["tests_revision"] == 2, "first list");
    w.list(&[A]).await?;
    let s = w.status().await?;
    ensure!(
        s["tests_revision"] == 3
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
    // checks-listed-first-ac4: criterion C has [A] at tests_revision 3, the revision of
    // its document.
    let w = world().await?;
    w.list(&[A]).await?;
    let s = w.status().await?;
    ensure!(listed(&s) == [A] && s["tests_revision"] == 3, "{s}");
    let (code, reply) = save_list(&w, &[A, B], Some(2)).await?;
    ensure!(
        code == 409 && reply["error"] == STALE_LIST,
        "{code} {reply}"
    );
    let s = w.status().await?;
    ensure!(listed(&s) == [A] && s["tests_revision"] == 3, "{s}");
    let (code, reply) = save_list(&w, &[A, B], Some(3)).await?;
    ensure!(code == 200, "{code} {reply}");
    let s = w.status().await?;
    ensure!(listed(&s) == [A, B] && s["tests_revision"] == 4, "{s}");
    let (code, reply) = save_list(&w, &[B], None).await?;
    ensure!(code == 200, "{code} {reply}");
    let s = w.status().await?;
    ensure!(listed(&s) == [B] && s["tests_revision"] == 5, "{s}");
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
    // A criterion without tests has an empty list at its document revision.
    let s = read().await?;
    ensure!(listed(&s).is_empty() && s["tests_revision"] == 1, "{s}");
    let save = |actor: &'static str, tests: Value| {
        p.write(
            actor,
            json!({"op":"set_ac_tests","target":"criterion:a","tests":tests,"revision":1,"reason":"Link the tests"}),
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
    ensure!(listed(&s) == [winner] && s["tests_revision"] == 2, "{s}");
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
    ensure!(status_before["tests_revision"] == 2, "{status_before}");
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
        listed(&before) == [B, A] && before["tests_revision"] == 4,
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
async fn a_red_test_stays_red_when_a_newer_record_omits_it() -> Result<()> {
    // defect-stays-red-ac1: opens fails on C1; a record for C2 has only refuses.
    let w = world().await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    w.record("c2c2c2c", outcomes(&[("refuses", "pass")]))
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
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    w.record(
        "c2c2c2c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
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
    // missing-is-not-pass-ac1: X passes on C1; the later record for C2 has X skipped.
    let w = world().await?;
    w.list(&[A]).await?;
    let s = w.status().await?;
    ensure!(
        s["tests_draft"] == false && s["tests"].as_array().map(Vec::len) == Some(1),
        "{s}"
    );
    w.record("c1c1c1c", outcomes(&[("opens", "pass")])).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == A
            && s["tests"][0]["status"] == "pass"
            && s["status"] == "green",
        "{s}"
    );
    w.record("c2c2c2c", outcomes(&[("opens", "skip")])).await?;
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
    // missing-is-not-pass-ac3: X passes on C1; the later record for C2 has X as todo.
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
    w.record("c1c1c1c", outcomes(&[("opens", "pass")])).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == A
            && s["tests"][0]["status"] == "pass"
            && s["status"] == "green",
        "{s}"
    );
    w.record("c2c2c2c", outcomes(&[("opens", "todo")])).await?;
    let s = w.status().await?;
    ensure!(
        s["tests"][0]["address"] == A && s["tests"][0]["status"] == "todo" && s["status"] == "grey",
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
    // A fresh criterion has an empty list at its first document revision.
    let s = read().await?;
    ensure!(s["tests"] == json!([]) && s["tests_revision"] == 1, "{s}");
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

/// Opens passes, and refuses fails with "expected 403, got 200".
fn failing_refuses() -> Value {
    json!([{"test":A,"status":"pass"},{"test":B,"status":"fail","message":"expected 403, got 200"}])
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn each_test_shows_its_status_commit_and_message() -> Result<()> {
    // old-results-history-ac1: agent bot reads A over HTTP with its own token.
    let w = world().await?;
    w.record("c1c1c1c", failing_refuses()).await?;
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
    w.record("c1c1c1c", failing_refuses()).await?;
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

/// Workspace W with criteria A1 to A101, and the token that the owner created for agent bot.
async fn many_criteria() -> Result<(World, String, Vec<String>)> {
    let w = world().await?;
    let mut acs = vec![json!({"id":"key-rights","text":"Only read and verify before launch"})];
    acs.extend(
        (1..=101).map(|i| json!({"id":format!("a{i}"),"text":format!("Criterion A{i} holds")})),
    );
    let command = json!({"op":"revise","req":"key","expected_revision":1,"capability":"restrict","title":"Key rights","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Many criteria","acs":acs});
    let (code, reply) = post(
        &w,
        &w.owner,
        "/commands",
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
    )
    .await?;
    ensure!(code == 200, "revise: {code} {reply}");
    let (code, created) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {created}");
    let bot = created["token"].as_str().unwrap().to_owned();
    let addresses = (1..=101).map(|i| format!("ac:a{i}")).collect();
    Ok((w, bot, addresses))
}

fn item_addresses(reply: &Value) -> Vec<&str> {
    reply["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["address"].as_str())
        .collect()
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn http_status_read_returns_all_101_criteria_in_request_order() -> Result<()> {
    // old-results-history-ac4: agent bot reads A1 to A101 in one HTTP request.
    let (w, bot, addresses) = many_criteria().await?;
    let reply = w
        .http
        .get(format!("{}/tests?ac={}", w.base, addresses.join(",")))
        .bearer_auth(&bot)
        .send()
        .await?;
    let code = reply.status().as_u16();
    let reply: Value = reply.json().await?;
    ensure!(code == 200, "{code} {reply}");
    ensure!(
        reply["items"].as_array().map(Vec::len) == Some(101),
        "item count: {reply}"
    );
    ensure!(item_addresses(&reply) == addresses, "{reply}");

    // Control: an unknown address at position 101 returns its error and no items.
    let mut listed = addresses[..100].to_vec();
    listed.push("ac:missing".into());
    let reply = w
        .http
        .get(format!("{}/tests?ac={}", w.base, listed.join(",")))
        .bearer_auth(&bot)
        .send()
        .await?;
    let code = reply.status().as_u16();
    let reply: Value = reply.json().await?;
    ensure!(
        code != 200
            && reply["items"].is_null()
            && reply["error"]
                .as_str()
                .is_some_and(|e| e.contains("unknown criterion ac:missing")),
        "{code} {reply}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn mcp_status_read_returns_all_101_criteria_in_request_order() -> Result<()> {
    // old-results-history-ac4: agent bot calls joppa_tests for A1 to A101 in one request.
    let (w, bot, addresses) = many_criteria().await?;
    let workspace = w.base.rsplit('/').next().unwrap();
    let reply = mcp_read(
        &w,
        &bot,
        "joppa_tests",
        json!({"workspace":workspace,"ac":addresses.join(",")}),
    )
    .await?;
    ensure!(
        reply["items"].as_array().map(Vec::len) == Some(101),
        "item count: {reply}"
    );
    ensure!(item_addresses(&reply) == addresses, "{reply}");

    // Control: an unknown address at position 101 returns its error and no items.
    let mut listed = addresses[..100].to_vec();
    listed.push("ac:missing".into());
    let reply = mcp(
        &w,
        &bot,
        "joppa_tests",
        json!({"workspace":workspace,"ac":listed.join(",")}),
    )
    .await?;
    ensure!(
        reply["result"]["isError"] == true
            && reply["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|e| e.contains("unknown criterion ac:missing")),
        "{reply}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn commits_behind_counts_newer_default_branch_commits() -> Result<()> {
    // time-not-stale-ac1: A is decided on C; five newer commits ran other tests only.
    let w = world().await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    for order in 2..=6 {
        let other = json!([{"test":"tests/other.test.ts > other","status":"pass"}]);
        w.record(
            &format!("{order}{order}{order}{order}{order}{order}{order}"),
            other,
        )
        .await?;
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
    // binding-pins-contract-ac1: "[ac:key-rights] other" fails in the same record.
    let w = world().await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(w.status().await?["status"] == "green", "green before");
    w.record(
        "c2c2c2c",
        outcomes(&[
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
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
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
    // A green criterion does not complete the Task and gives it no commit.
    ensure!(
        t["done"] == false && t["commit"].is_null() && t.get("verification").is_none(),
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
            json!({"op":"complete_task","commit":"c0ffee0","id":"task:finished","revision":2,"evidence":"Implemented the contract"}),
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
    for (commit, verification, cases) in [
        (None, "unverified", vec![]),
        (
            Some("c1c1c1c"),
            "failed",
            vec![("opens", "fail"), ("refuses", "pass")],
        ),
        (
            Some("c2c2c2c"),
            "passed",
            vec![("opens", "pass"), ("refuses", "pass")],
        ),
    ] {
        if let Some(commit) = commit {
            w.record(commit, outcomes(&cases)).await?;
        }
        // Only complete_task sets done and commit; records change neither.
        let _ = verification;
        let finished = read("task:finished").await?;
        ensure!(
            finished["done"] == true && finished["commit"] == "c0ffee0",
            "{finished}"
        );
        let open = read("task:key/t1").await?;
        ensure!(open["done"] == false && open["commit"].is_null(), "{open}");
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
        description.contains("Only complete_task sets done and commit")
            && !description.contains("verification follows the criterion status")
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
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(task(&w).await?["done"] == false, "not completed on c1");
    w.record(
        "c2c2c2c",
        outcomes(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    let t = task(&w).await?;
    ensure!(
        t["done"] == false && t["commit"].is_null() && t.get("verification").is_none(),
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
            json!({"op":"complete_task","commit":"c0ffee0","id":"task:finished","revision":2,"evidence":"Implemented the contract"}),
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
    for (commit, verification, status, cases) in [
        (
            "c1c1c1c",
            "passed",
            "green",
            [("opens", "pass"), ("refuses", "pass")],
        ),
        (
            "c2c2c2c",
            "failed",
            "red",
            [("opens", "fail"), ("refuses", "pass")],
        ),
    ] {
        w.record(commit, outcomes(&cases)).await?;
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
        // The Task keeps its own state and commit; the criterion status does not change it.
        let _ = (verification, status);
        for (via, t) in [("HTTP", &http), ("MCP", &mcp)] {
            ensure!(
                t["done"] == true && t["commit"] == "c0ffee0" && t.get("verification").is_none(),
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

// removal-at-once-ac2
// Owner S revokes the write token of bot. The first call after that is a read, and it fails.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_revoked_write_token_fails_on_the_first_read_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let workspace = w.base.rsplit('/').next().unwrap().to_owned();
    let root = w.base.split("/api/").next().unwrap().to_owned();
    let read_http = |token: String| {
        let request = w
            .http
            .get(format!("{}/objects?address=note:from-bot", w.base))
            .bearer_auth(token);
        async move {
            let reply = request.send().await?;
            anyhow::Ok((
                reply.status().as_u16(),
                reply.json::<Value>().await.unwrap_or(Value::Null),
            ))
        }
    };
    let read_mcp = |token: String| {
        let request = w.http.post(format!("{root}/mcp")).bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":workspace,"address":"note:from-bot"}}}));
        async move {
            let reply = request.send().await?;
            anyhow::Ok((
                reply.status().as_u16(),
                reply.json::<Value>().await.unwrap_or(Value::Null),
            ))
        }
    };
    for mcp in [false, true] {
        let (code, created) = post(
            &w,
            &w.owner,
            "/agent-tokens",
            json!({"agent":"bot","access":"write"}),
        )
        .await?;
        ensure!(
            code == 200 && created["access"] == "write" && created["agent"] == "bot",
            "{code} {created}"
        );
        let bot = created["token"].as_str().unwrap_or_default().to_owned();
        // Controls: the write grant writes, and the token reads.
        if !mcp {
            let (code, saved) = save_note(&w, &bot, "from-bot").await?;
            ensure!(code == 200, "{code} {saved}");
        }
        let (code, read) = read_http(bot.clone()).await?;
        ensure!(
            code == 200 && read["address"] == "note:from-bot",
            "{code} {read}"
        );
        let (code, read) = read_mcp(bot.clone()).await?;
        ensure!(
            code == 200 && read["result"]["isError"] != true,
            "{code} {read}"
        );
        let id = created["id"].as_i64().unwrap_or_default();
        let (code, reply) = post(
            &w,
            &w.owner,
            &format!("/agent-tokens/{id}/revoke"),
            json!({}),
        )
        .await?;
        ensure!(code == 200, "revoke: {code} {reply}");
        let (code, reply) = if mcp {
            read_mcp(bot).await?
        } else {
            read_http(bot).await?
        };
        ensure!(
            code == 401 && reply["error"] == "token revoked",
            "first read after the revocation, MCP={mcp}: {code} {reply}"
        );
    }
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

// read-only-agent-token-ac3
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
        "id",
        "revoked",
        "revoked_at",
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
                && item["created_at"].is_string()
                && item["revoked"] == false
                && item["revoked_at"].is_null()
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

/// A cookie-session token call to the workspace of `w`, with an optional Origin header.
async fn session_post(
    w: &World,
    session: &str,
    origin: Option<&str>,
    path: &str,
    body: Value,
) -> Result<(u16, Value)> {
    let mut request = w
        .http
        .post(format!("{}{path}", w.base))
        .header("cookie", format!("joppa_session={session}"))
        .json(&body);
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    let reply = request.send().await?;
    Ok((
        reply.status().as_u16(),
        reply.json().await.unwrap_or(Value::Null),
    ))
}

// settings-in-name-menu-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn settings_in_name_menu_ac2_access_lists_people_by_name_and_no_agents() -> Result<()> {
    let w = world().await?;
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let ws = w.base.rsplit('/').next().context("workspace")?.to_owned();
    let root = w.base.split("/api/").next().context("root")?.to_owned();
    let person = format!("person-{}", uuid::Uuid::new_v4().simple());
    let op: Operation = serde_json::from_value(json!({"op":"member","id":person,"role":"human"}))?;
    store::execute(&pool, &ws, "owner", "add-person", &op).await?;
    let email = format!("{person}@example.com");
    pool.get()
        .await?
        .execute(
            "INSERT INTO joppa.google_identities(actor,google_sub,email,display_name) VALUES($1,$1,$2,'Maria Lopez')",
            &[&person, &email],
        )
        .await?;
    // The grant of a member is not listed again; a pending grant is.
    let pending = format!("pending-{person}@example.com");
    store::grant_email(&pool, &ws, "owner", &email).await?;
    store::grant_email(&pool, &ws, "owner", &pending).await?;
    let read = |token: String| {
        let http = w.http.clone();
        let url = format!("{root}/api/access");
        async move {
            let reply = http.get(url).bearer_auth(token).send().await?;
            anyhow::Ok((reply.status().as_u16(), reply.json::<Value>().await?))
        }
    };
    let (code, owned) = read(w.owner.clone()).await?;
    ensure!(code == 200, "{code} {owned}");
    let item = owned["items"]
        .as_array()
        .and_then(|items| items.iter().find(|i| i["id"] == ws.as_str()))
        .context("owned workspace")?;
    let people: Vec<&str> = item["people"]
        .as_array()
        .context("people")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    // A person with a Google identity shows by name and email; the agent member is absent.
    ensure!(
        people.contains(&format!("Maria Lopez <{email}>").as_str()) && !people.contains(&"agent"),
        "{people:?}"
    );
    ensure!(item["emails"] == json!([pending]), "{}", item["emails"]);
    // A member who does not own the workspace gets no entry for it.
    let (code, other) = read(w.other.clone()).await?;
    ensure!(code == 200, "{code} {other}");
    ensure!(
        other["items"]
            .as_array()
            .is_some_and(|items| items.iter().all(|i| i["id"] != ws.as_str())),
        "{other}"
    );
    Ok(())
}

// settings-agent-tokens-ac1, fixture 2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn settings_agent_tokens_ac1_session_issue_list_and_origin() -> Result<()> {
    let w = world().await?;
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let session = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&pool, &session, "owner", None).await?;
    let root = w.base.split("/api/").next().context("root")?.to_owned();
    let ws = w.base.rsplit('/').next().context("workspace")?.to_owned();
    // Token and revocation rows of W.
    let counts = || async {
        let row = pool
            .get()
            .await?
            .query_one(
                "SELECT (SELECT count(*) FROM joppa.agent_tokens WHERE workspace=$1),
                        (SELECT count(*) FROM joppa.agent_token_revocations r
                         JOIN joppa.agent_tokens t ON t.id=r.token_id WHERE t.workspace=$1)",
                &[&ws],
            )
            .await?;
        anyhow::Ok((row.get::<_, i64>(0), row.get::<_, i64>(1)))
    };
    let reader = json!({"agent":"planner-reader","access":"read"});

    // A cross-origin cookie-session create is refused and stores nothing.
    let before = counts().await?;
    let (code, reply) = session_post(
        &w,
        &session,
        Some("http://evil.example"),
        "/agent-tokens",
        reader.clone(),
    )
    .await?;
    ensure!(
        code == 403 && reply["error"] == "cross-origin request denied",
        "{code} {reply}"
    );
    ensure!(
        counts().await? == before,
        "a cross-origin create stored data"
    );

    // Fixture 2: a blank agent name issues no token.
    let (_, reply) = session_post(
        &w,
        &session,
        Some(&root),
        "/agent-tokens",
        json!({"agent":"","access":"read"}),
    )
    .await?;
    ensure!(
        reply["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("invalid id")),
        "{reply}"
    );
    ensure!(counts().await? == before, "a blank agent stored data");

    // The owner issues a read token from a same-origin cookie session.
    let (code, issued) = session_post(&w, &session, Some(&root), "/agent-tokens", reader).await?;
    ensure!(
        code == 200 && issued["agent"] == "planner-reader" && issued["access"] == "read",
        "{code} {issued}"
    );
    let token = issued["token"].as_str().context("token")?.to_owned();
    ensure!(counts().await? == (before.0 + 1, before.1));

    // The read token reads W, and a write gets the read-only refusal.
    let read = w
        .http
        .get(format!("{}/objects?parent=*", w.base))
        .bearer_auth(&token)
        .send()
        .await?;
    ensure!(read.status().is_success(), "{}", read.status());
    let (code, refused) = save_note(&w, &token, "refused-by-reader").await?;
    ensure!(
        code == 403 && refused["error"] == "read-only token",
        "{code} {refused}"
    );

    // A bearer create with no Origin header issues a token as before.
    let (code, bot) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200 && bot["access"] == "write", "{code} {bot}");
    let bot_id = bot["id"].as_i64().context("id")?;

    // A cross-origin cookie revoke is refused; a same-origin one revokes.
    let revoke = format!("/agent-tokens/{bot_id}/revoke");
    let before = counts().await?;
    let (code, reply) = session_post(
        &w,
        &session,
        Some("http://evil.example"),
        &revoke,
        Value::Null,
    )
    .await?;
    ensure!(
        code == 403 && reply["error"] == "cross-origin request denied",
        "{code} {reply}"
    );
    ensure!(
        counts().await? == before,
        "a cross-origin revoke stored data"
    );
    let (code, reply) = session_post(&w, &session, Some(&root), &revoke, Value::Null).await?;
    ensure!(code == 200 && reply["revoked"] == true, "{code} {reply}");

    // The owner list holds exactly the public fields, with the stored dates.
    let reply = w
        .http
        .get(format!("{}/agent-tokens", w.base))
        .bearer_auth(&w.owner)
        .send()
        .await?;
    ensure!(reply.status().as_u16() == 200, "{}", reply.status());
    let listed: Value = reply.json().await?;
    let rows = pool
        .get()
        .await?
        .query(
            "SELECT t.id, t.token_hash, t.created_at::text, r.revoked_at::text
             FROM joppa.agent_tokens t
             LEFT JOIN joppa.agent_token_revocations r ON r.token_id=t.id
             WHERE t.workspace=$1",
            &[&ws],
        )
        .await?;
    let text = listed.to_string();
    ensure!(!text.contains(&token), "the list holds a token value");
    let items = listed["items"].as_array().context("items")?;
    ensure!(items.len() == rows.len(), "{listed}");
    for row in &rows {
        let (id, hash): (i64, String) = (row.get(0), row.get(1));
        ensure!(!text.contains(&hash), "the list holds a token hash");
        let item = items
            .iter()
            .find(|i| i["id"] == id)
            .with_context(|| format!("{id} not listed: {listed}"))?;
        let mut keys: Vec<&str> = item
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, _)| k.as_str())
            .collect();
        keys.sort_unstable();
        let revoked_at: Option<String> = row.get(3);
        ensure!(
            keys == [
                "access",
                "agent",
                "created_at",
                "id",
                "revoked",
                "revoked_at"
            ] && item["created_at"] == row.get::<_, String>(2)
                && item["revoked_at"] == json!(revoked_at)
                && item["revoked"] == revoked_at.is_some(),
            "{item}"
        );
    }
    let find = |agent: &str| items.iter().find(|i| i["agent"] == agent);
    ensure!(
        find("planner-reader").is_some_and(|i| i["access"] == "read" && i["revoked"] == false)
            && find("bot").is_some_and(|i| i["access"] == "write" && i["revoked"] == true),
        "{listed}"
    );
    Ok(())
}

// settings-agent-tokens-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn settings_agent_tokens_ac3_a_member_session_cannot_issue_or_revoke() -> Result<()> {
    let w = world().await?;
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    let root = w.base.split("/api/").next().context("root")?.to_owned();
    let ws = w.base.rsplit('/').next().context("workspace")?.to_owned();
    // alex is a human member of W, not its owner, with a cookie session.
    let member: Operation =
        serde_json::from_value(json!({"op":"member","id":"alex","role":"human"}))?;
    store::execute(&pool, &ws, "owner", "ac3-member-alex", &member).await?;
    let session = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    sessions::create(&pool, &session, "alex", None).await?;
    // The owner issues the token of bot.
    let (code, bot) = post(&w, &w.owner, "/agent-tokens", json!({"agent":"bot"})).await?;
    ensure!(code == 200, "{code} {bot}");
    let bot_id = bot["id"].as_i64().context("id")?;
    let token = bot["token"].as_str().context("token")?.to_owned();
    // Token and revocation rows of W.
    let counts = || async {
        let row = pool
            .get()
            .await?
            .query_one(
                "SELECT (SELECT count(*) FROM joppa.agent_tokens WHERE workspace=$1),
                        (SELECT count(*) FROM joppa.agent_token_revocations r
                         JOIN joppa.agent_tokens t ON t.id=r.token_id WHERE t.workspace=$1)",
                &[&ws],
            )
            .await?;
        anyhow::Ok((row.get::<_, i64>(0), row.get::<_, i64>(1)))
    };
    let before = counts().await?;
    ensure!(before == (1, 0), "{before:?}");

    // A same-origin create as alex is refused and stores no token.
    let (code, reply) = session_post(
        &w,
        &session,
        Some(&root),
        "/agent-tokens",
        json!({"agent":"helper","access":"read"}),
    )
    .await?;
    ensure!(
        code == 409 && reply["error"] == "workspace owner required",
        "{code} {reply}"
    );
    ensure!(counts().await? == before, "a member create stored data");

    // A same-origin revoke of the token of bot as alex is refused and stores no revocation.
    let (code, reply) = session_post(
        &w,
        &session,
        Some(&root),
        &format!("/agent-tokens/{bot_id}/revoke"),
        Value::Null,
    )
    .await?;
    ensure!(
        code == 409 && reply["error"] == "workspace owner required",
        "{code} {reply}"
    );
    ensure!(counts().await? == before, "a member revoke stored data");

    // The token of bot still reads W.
    let read = w
        .http
        .get(format!("{}/objects?parent=*", w.base))
        .bearer_auth(&token)
        .send()
        .await?;
    ensure!(read.status().is_success(), "{}", read.status());
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

const NEEDS: &str = "acceptance needs records on the commit where every listed test passes";

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
async fn an_acceptance_on_a_green_record_names_its_commit_and_actor() -> Result<()> {
    // acceptance-names-subject-ac1, acceptance-shows-actor-ac1
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
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
async fn a_failing_test_on_the_commit_refuses_acceptance() -> Result<()> {
    // accept-needs-all-passed-ac1
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "fail"), ("refuses", "pass")]),
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
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
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
async fn an_acceptance_without_a_commit_is_refused() -> Result<()> {
    // acceptance-names-subject-ac2
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
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
async fn a_skipped_test_on_the_commit_refuses_acceptance() -> Result<()> {
    // acceptance-names-subject-ac3
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "skip")]),
    )
    .await?;
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_cannot_accept_a_requirement() -> Result<()> {
    // decisions-from-people-ac1
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
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
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    refused_acceptance(&w, "c1c1c1c", NEEDS).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_listed_test_without_a_record_on_the_commit_refuses_acceptance() -> Result<()> {
    // accept-needs-all-passed-ac1: a listed test that did not run is not a pass.
    let w = world().await?;
    w.list(&[A, B]).await?;
    w.record("c1c1c1c", outcomes(&[("opens", "pass")])).await?;
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
    command(&w,json!({"op":"save_object","kind":"check","id":"configured","revision":0,"parent":"requirement:key","data":{"title":"The configured result passes","description":"Check the configured contract","owner":"owner","consumer":"customer","depends_on":[],"before_launch":false,"decision_required":false},"reason":"Create a configured criterion"})).await?;
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
    for (commit, expected, verification, cases) in [
        (None, "grey", "unverified", vec![]),
        (
            Some("c1c1c1c"),
            "green",
            "passed",
            vec![("opens", "pass"), ("refuses", "pass")],
        ),
        (
            Some("c2c2c2c"),
            "red",
            "failed",
            vec![("opens", "fail"), ("refuses", "pass")],
        ),
    ] {
        if let Some(commit) = commit {
            w.record(commit, outcomes(&cases)).await?;
        }
        for (address, criterion) in tasks {
            // A configured Task role reads its own state: the criterion status does not
            // reach the Task.
            let _ = (criterion, expected, verification);
            let status = read(&w, bot, address).await?;
            ensure!(
                status["address"] == address
                    && status["done"] == false
                    && status["commit"].is_null()
                    && status.get("verification").is_none(),
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

const NO_TASK_CHANGES: &str = "planning produced no Task changes";

impl Planning<'_> {
    /// AC6 fixture: A is ready and grey with X listed; Y passes in a record. P claims A.
    async fn bound_plan(&self) -> Result<()> {
        // check-list-frozen-ac3: a list save clears readiness, so A is assessed after it.
        self.write(
            "agent",
            json!({"op":"set_ac_tests","target":"criterion:a","tests":[A],"reason":"Link the tests"}),
        )
        .await?;
        self.ready().await?;
        self.record("c1c1c1c", outcomes(&[("refuses", "pass")]))
            .await?;
        let status = self.tests().await?;
        ensure!(
            status["status"] == "grey" && listed(&status) == [A],
            "{status}"
        );
        self.claim("agent", "criterion:a").await
    }
    /// GET /tests?ac=criterion:a.
    async fn tests(&self) -> Result<Value> {
        let root = self.w.base.split("/api/").next().unwrap();
        Ok(self
            .w
            .http
            .get(format!(
                "{root}/api/workspaces/{}/tests?ac=criterion:a",
                self.ws
            ))
            .bearer_auth(&self.w.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    /// Addresses of the saved objects.
    async fn addresses(&self) -> Result<Vec<Value>> {
        Ok(self.saved().await?["objects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["address"].clone())
            .collect())
    }
}

fn save_xy() -> Value {
    json!({"op":"set_ac_tests","target":"criterion:a","tests":[A,B],"reason":"Bind the existing tests"})
}

/// Saved rows without the planning claim, the journal and its derived rows.
fn without_claim(mut saved: Value) -> Value {
    for table in ["events", "command_receipts", "board_cache", "position"] {
        saved[table] = Value::Null;
    }
    for table in ["state_entries", "state_revisions"] {
        saved[table]
            .as_array_mut()
            .unwrap()
            .retain(|row| row["section"] != "planning_claims");
    }
    for object in saved["objects"].as_array_mut().unwrap() {
        if object["process"].is_object() {
            object["process"]["planning_claim"] = Value::Null;
        }
    }
    saved
}

// planning-claim-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac6_fixture1_a_holder_test_list_save_finishes_with_no_task_and_replays() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.bound_plan().await?;
        let objects = p.addresses().await?;
        p.send(mcp, &save_xy()).await?.map_err(anyhow::Error::msg)?;
        p.send(mcp, &finish_command())
            .await?
            .map_err(anyhow::Error::msg)?;
        let a = p.read("criterion:a").await?;
        ensure!(a["process"]["planning_claim"].is_null(), "{a}");
        ensure!(p.addresses().await? == objects, "finish created a Task");
        ensure!(listed(&p.tests().await?) == [A, B]);
        store::check_journal(&p.pool).await?;
        p.pool
            .get()
            .await?
            .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&p.ws])
            .await?;
        store::migrate(&p.pool).await?;
        ensure!(p.read("criterion:a").await?["process"]["planning_claim"].is_null());
        ensure!(listed(&p.tests().await?) == [A, B]);
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// planning-claim-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac6_fixture2_a_finish_without_changes_is_refused_and_the_holder_releases() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.bound_plan().await?;
        p.refuse_command("no change", finish_command(), NO_TASK_CHANGES)
            .await?;
        ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "agent");
        ensure!(listed(&p.tests().await?) == [A]);
        let before = p.saved().await?;
        p.send(
            mcp,
            &json!({"op":"release_planning","ac":"criterion:a","reason":"Nothing to plan"}),
        )
        .await?
        .map_err(anyhow::Error::msg)?;
        let after = p.saved().await?;
        ensure!(p.read("criterion:a").await?["process"]["planning_claim"].is_null());
        ensure!(
            without_claim(after) == without_claim(before),
            "release changed more than the claim"
        );
    }
    Ok(())
}

// planning-claim-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac6_fixture3_a_test_list_save_by_another_member_does_not_finish() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.bound_plan().await?;
    p.write("other", save_xy()).await?;
    p.refuse_command("save by Q", finish_command(), NO_TASK_CHANGES)
        .await?;
    ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "agent");
    ensure!(listed(&p.tests().await?) == [A, B]);
    Ok(())
}

// planning-claim-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac6_fixture4_a_refused_batch_rolls_back_the_save_and_the_finish() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.bound_plan().await?;
    let before = p.saved().await?;
    let response = p
        .mcp(
            "joppa_batch",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"operations":[
                {"command":save_xy()},
                {"command":finish_command()},
                {"command":{"op":"set_ac_tests","target":"criterion:missing","tests":[A],"reason":"Refused"}}
            ]}),
        )
        .await?;
    ensure!(
        response["result"]["isError"] == true
            && response["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|e| e.ends_with("unknown criterion")),
        "{response}"
    );
    let after = p.saved().await?;
    ensure!(
        after == before && after["position"] == before["position"],
        "the refused batch changed saved state"
    );
    ensure!(p.read("criterion:a").await?["process"]["planning_claim"]["actor"] == "agent");
    ensure!(listed(&p.tests().await?) == [A]);
    Ok(())
}

const AWAITING: &str = "Task is awaiting planning publication";
const HOLDER_REQUIRED: &str = "planning holder required";
const NO_CLAIM: &str = "criterion has no planning claim";
const EMPTY_TEXT: &str = "text must not be empty";

impl Planning<'_> {
    /// The `token` member reads `path` over HTTP and through the MCP `tool`.
    async fn reads(
        &self,
        token: &str,
        path: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<[(&'static str, Value); 2]> {
        let root = self.w.base.split("/api/").next().unwrap();
        let http: Value = self
            .w
            .http
            .get(format!("{root}/api/workspaces/{}/{path}", self.ws))
            .bearer_auth(token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mcp = self.mcp_as(token, tool, arguments).await?;
        ensure!(mcp["result"]["isError"] != true, "MCP {tool}: {mcp}");
        let mcp = serde_json::from_str(
            mcp["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        Ok([("HTTP", http), ("MCP", mcp)])
    }
    /// One Task view of the `token` member over HTTP and MCP.
    async fn view(&self, token: &str, view: &str) -> Result<[(&'static str, Value); 2]> {
        self.reads(
            token,
            &format!("tasks?view={view}&limit=100"),
            "joppa_tasks",
            json!({"view":view,"limit":100}),
        )
        .await
    }
    /// The view=all entry of `address` for the `token` member over HTTP and MCP.
    async fn entry(&self, token: &str, address: &str) -> Result<[(&'static str, Value); 2]> {
        let [(http, h), (mcp, m)] = self.view(token, "all").await?;
        let find = |page: Value| {
            queued(&page, address)
                .ok_or_else(|| anyhow::anyhow!("{address} is missing in view=all: {page}"))
        };
        Ok([(http, find(h)?), (mcp, find(m)?)])
    }
    /// One object read of the `token` member over HTTP and MCP.
    async fn object(&self, token: &str, address: &str) -> Result<[(&'static str, Value); 2]> {
        self.reads(
            token,
            &format!("objects?address={address}"),
            "joppa_objects",
            json!({"address":address}),
        )
        .await
    }
    /// `actor` sends `command` through the store and gets its journal position.
    async fn position(&self, actor: &str, command: Value) -> Result<u64> {
        let reply = self.write(actor, command).await?;
        reply["position"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("reply has no position: {reply}"))
    }
    /// The saved revisions of `address`, in revision order.
    async fn history(&self, address: &str) -> Result<Vec<Value>> {
        let mut rows: Vec<Value> = self.saved().await?["object_revisions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["address"] == address)
            .cloned()
            .collect();
        rows.sort_by_key(|r| r["revision"].as_u64());
        Ok(rows)
    }
    /// The workspace owner saves A with `description` and confirms it ready again.
    async fn revise_a(&self, description: &str) -> Result<u64> {
        self.save(
            "criterion:a",
            Some("spec:one"),
            json!({"title":"criterion:a","owner":"owner","description":description}),
            json!([]),
        )
        .await?;
        self.ready().await?;
        self.revision("criterion:a").await
    }
    /// The view=all values of T for P: each transport must satisfy `check`.
    async fn expect_t(&self, point: &str, check: impl Fn(&Value) -> bool) -> Result<()> {
        for (transport, t) in self.entry(&self.w.token, "work:t").await? {
            ensure!(check(&t), "{point} {transport}: {t}");
        }
        Ok(())
    }
    /// AC7 fixture: T with `links` is planned at A revision 4. A is at revision 5,
    /// so T needs review. P claims A and saves T with a new contract.
    async fn replanned(&self, links: Value) -> Result<()> {
        let mut revision = self.revision("criterion:a").await?;
        while revision < 4 {
            revision = self.revise_a(&format!("Wording {revision}")).await?;
        }
        ensure!(revision == 4, "A revision {revision}");
        self.save("work:t", Some("criterion:a"), json!({"title":"T","owner":"owner","contract":"Implement this criterion","purpose":"implementation"}), links.clone()).await?;
        self.expect_t("at A revision 4", |t| t["review_required"] == false)
            .await?;
        ensure!(self.revise_a("Fifth wording").await? == 5);
        self.expect_t("at A revision 5", |t| t["review_required"] == true)
            .await?;
        self.claim("agent", "criterion:a").await?;
        self.expect_t("after the claim", |t| t["review_required"] == true)
            .await?;
        self.plan_task(
            "work:t",
            "Implement the criterion with a new contract",
            links,
        )
        .await?;
        self.expect_t("after the holder save", |t| t["review_required"] == true)
            .await
    }
    /// AC3 fixture: W (`worker`) holds T0 under ready A, and P holds A.
    async fn held_t0(&self) -> Result<()> {
        self.ready().await?;
        self.t0().await?;
        self.write(
            "owner",
            json!({"op":"enroll_service","id":"worker","role":"agent"}),
        )
        .await?;
        let revision = self.revision("work:t0").await?;
        self.write(
            "worker",
            json!({"op":"claim_task","id":"work:t0","revision":revision}),
        )
        .await?;
        self.claim("agent", "criterion:a").await
    }
    /// P saves T0 of W with a new contract.
    async fn update_t0(&self) -> Result<()> {
        let revision = self.revision("work:t0").await?;
        self.write("agent", json!({"op":"save_object","kind":"work","id":"t0","revision":revision,"parent":"criterion:a","data":{"title":"T0","owner":"owner","contract":"Implement the updated plan","purpose":"implementation"},"reason":"Update the older work"})).await?;
        Ok(())
    }
    /// The planning claim of `ac` on both transports.
    async fn planning_claims(&self, ac: &str) -> Result<Vec<(&'static str, Value)>> {
        Ok(self
            .object(&self.w.token, ac)
            .await?
            .into_iter()
            .map(|(transport, a)| (transport, a["process"]["planning_claim"].clone()))
            .collect())
    }
    /// P still holds A, and each new Task is unavailable for publication.
    async fn unpublished(&self, tasks: &[&str]) -> Result<()> {
        for (transport, claim) in self.planning_claims("criterion:a").await? {
            ensure!(claim["actor"] == "agent", "{transport}: {claim}");
        }
        for task in tasks {
            for (transport, t) in self.entry(&self.w.token, task).await? {
                ensure!(
                    t["claimable"] == false && t["waiting_reason"] == AWAITING,
                    "{transport} {task}: {t}"
                );
            }
        }
        Ok(())
    }
}

/// The entry of `address` on one Task view page.
fn queued(page: &Value, address: &str) -> Option<Value> {
    page["tasks"]
        .as_array()
        .and_then(|tasks| tasks.iter().find(|t| t["address"] == address))
        .cloned()
}

fn release_command(reason: &str) -> Value {
    json!({"op":"release_planning","ac":"criterion:a","reason":reason})
}

/// Delete the board cache and rebuild the projections from the journal.
async fn rebuild(p: &Planning<'_>) -> Result<()> {
    p.pool
        .get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&p.ws])
        .await?;
    store::migrate(&p.pool).await
}

// planning-claim-ac7
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac7_a_replanned_task_follows_the_current_criterion_revision_after_finish() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.replanned(json!([])).await?;
        p.finish(mcp).await?;
        let published = |t: &Value| {
            t["review_required"] == false && t["claimable"] == true && t["waiting_reason"].is_null()
        };
        p.expect_t("after finish", published).await?;
        store::check_journal(&p.pool).await?;
        rebuild(&p).await?;
        p.expect_t("after replay", published).await?;
        ensure!(p.revise_a("Sixth wording").await? == 6);
        p.expect_t("at A revision 6", |t| t["review_required"] == true)
            .await?;
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// planning-claim-ac7
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac7_finish_keeps_the_wait_for_a_changed_blocking_dependency() -> Result<()> {
    let w = world().await?;
    let dependency = |contract: &str| json!({"title":"D","owner":"owner","contract":contract,"purpose":"implementation"});
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.save(
            "work:d",
            Some("criterion:b"),
            dependency("Deliver the dependency"),
            json!([]),
        )
        .await?;
        p.replanned(
            json!([{"target":"work:d","relation":"depends_on","revision":1,"blocking":true}]),
        )
        .await?;
        p.save(
            "work:d",
            Some("criterion:b"),
            dependency("Deliver the changed dependency"),
            json!([]),
        )
        .await?;
        let before = p.object(&w.token, "work:t").await?;
        for (transport, t) in &before {
            ensure!(t["review_required"] == true, "{transport}: {t}");
        }
        p.finish(mcp).await?;
        p.expect_t("after finish", |t| {
            t["review_required"] == false
                && t["claimable"] == false
                && t["waiting_reason"]
                    == "Task has an unmet dependency or a changed dependency revision: work:d"
                && t["signal"]["label"] == "Dependency revision changed"
        })
        .await?;
        // Finish keeps the dependency pin and the dependency review.
        let after = p.object(&w.token, "work:t").await?;
        for ((transport, old), (_, new)) in before.iter().zip(&after) {
            ensure!(
                new["document"]["links"] == old["document"]["links"]
                    && new["review_required"] == true
                    && new["signal"]["label"] == "Dependency revision changed",
                "{transport}: {old} {new}"
            );
        }
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// planning-claim-ac7
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac7_a_refused_finish_and_a_release_keep_the_old_criterion_pin() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.replanned(json!([])).await?;
    // The refusal compares every saved row, the journal position included.
    p.refuse_command_as(&w.other, "Q finishes A", finish_command(), HOLDER_REQUIRED)
        .await?;
    p.expect_t("after the refused finish", |t| t["review_required"] == true)
        .await?;
    for (transport, claim) in p.planning_claims("criterion:a").await? {
        ensure!(claim["actor"] == "agent", "{transport}: {claim}");
    }
    p.send(false, &release_command("Plan withdrawn"))
        .await?
        .map_err(anyhow::Error::msg)?;
    p.expect_t("after the release", |t| t["review_required"] == true)
        .await?;
    store::check_journal(&p.pool).await?;
    Ok(())
}

// planning-claim-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac2_planning_hides_only_the_new_tasks_of_its_holder_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.ready().await?;
        p.t0().await?;
        let revision = p.revision("criterion:a").await?;
        let claim = p
            .position(
                "agent",
                json!({"op":"claim_planning","ac":"criterion:a","revision":revision}),
            )
            .await?;
        let mut created = vec![];
        for id in ["t1", "t2"] {
            let position = p.position("agent", json!({"op":"save_object","kind":"work","id":id,"revision":0,"parent":"criterion:a","data":{"title":id,"owner":"owner","contract":"Implement this criterion","purpose":"implementation"},"reason":"Plan the work"})).await?;
            created.push((format!("work:{id}"), position));
        }
        ensure!(claim < created[0].1 && created[0].1 < created[1].1);

        // Q reads all and available Tasks.
        for view in ["all", "available"] {
            for (transport, page) in p.view(&w.other, view).await? {
                for (task, _) in &created {
                    let t = queued(&page, task);
                    if view == "available" {
                        ensure!(t.is_none(), "{transport} available lists {task}: {page}");
                    } else {
                        ensure!(
                            t.as_ref().is_some_and(|t| t["status"] == "open"
                                && t["assignee"].is_null()
                                && t["claimable"] == false
                                && t["waiting_reason"] == AWAITING),
                            "{transport} all {task}: {page}"
                        );
                    }
                }
                ensure!(
                    queued(&page, "work:t0").is_some_and(|t| t["claimable"] == true),
                    "{transport} {view} T0: {page}"
                );
            }
        }
        for (task, _) in &created {
            let revision = p.revision(task).await?;
            p.refuse_command_as(
                &w.other,
                &format!("Q claims {task}"),
                json!({"op":"claim_task","id":task,"revision":revision}),
                AWAITING,
            )
            .await?;
        }

        // claim_next_task selects T0, and then no Task is left.
        p.send_as(&w.other, mcp, &json!({"op":"claim_next_task"}))
            .await?
            .map_err(anyhow::Error::msg)?;
        for (transport, t0) in p.object(&w.other, "work:t0").await? {
            ensure!(
                t0["process"]["status"] == "in_progress" && t0["process"]["assignee"] == "other",
                "mcp={mcp} {transport}: {t0}"
            );
        }
        p.refuse_command_as(
            &w.other,
            "claim_next_task with only T1 and T2",
            json!({"op":"claim_next_task"}),
            NO_MATCH,
        )
        .await?;

        for (task, position) in &created {
            for (transport, t) in p.object(&w.other, task).await? {
                ensure!(
                    t["process"]["status"] == "open"
                        && t["process"]["assignee"].is_null()
                        && t["process"]["created_by"] == "agent"
                        && t["process"]["created_at"] == *position
                        && t["document"]["parent"] == "criterion:a",
                    "{transport} {task}: {t}"
                );
            }
        }
        for (transport, planning_claim) in p.planning_claims("criterion:a").await? {
            ensure!(
                planning_claim == json!({"actor":"agent","position":claim}),
                "{transport}: {planning_claim}"
            );
        }
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// planning-claim-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac3_a_valid_creation_plan_publishes_its_tasks_and_each_refusal_keeps_the_claim()
-> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.held_t0().await?;
        p.plan_task("work:t1", "Implement this criterion", json!([]))
            .await?;
        p.plan_task(
            "work:t2",
            "Implement the dependent work",
            json!([{"target":"work:t1","relation":"depends_on","revision":1,"blocking":true}]),
        )
        .await?;
        let verification = p.read("criterion:a").await?["process"]["verification"].clone();
        let new = ["work:t1", "work:t2"];
        p.refuse_command_as(&w.other, "Q finishes A", finish_command(), HOLDER_REQUIRED)
            .await?;
        p.unpublished(&new).await?;
        let blank = json!({"op":"finish_planning","ac":"criterion:a","reason":" "});
        p.refuse_command("blank reason", blank, EMPTY_TEXT).await?;
        p.unpublished(&new).await?;

        p.finish(mcp).await?;
        for (transport, claim) in p.planning_claims("criterion:a").await? {
            ensure!(claim.is_null(), "mcp={mcp} {transport}: {claim}");
        }
        ensure!(p.read("criterion:a").await?["process"]["verification"] == verification);
        for (transport, t0) in p.object(&w.token, "work:t0").await? {
            ensure!(
                t0["process"]["status"] == "in_progress" && t0["process"]["assignee"] == "worker",
                "{transport}: {t0}"
            );
        }
        for (transport, t) in p.entry(&w.token, "work:t1").await? {
            ensure!(
                t["claimable"] == true && t["waiting_reason"].is_null(),
                "{transport}: {t}"
            );
        }
        for (transport, t) in p.entry(&w.token, "work:t2").await? {
            ensure!(
                t["claimable"] == false
                    && t["waiting_reason"]
                        == "Task has an unmet dependency or a changed dependency revision: work:t1",
                "{transport}: {t}"
            );
        }
        for (transport, page) in p.view(&w.token, "available").await? {
            ensure!(
                queued(&page, "work:t1").is_some() && queued(&page, "work:t2").is_none(),
                "{transport} available: {page}"
            );
        }
        p.refuse_command("no active claim", finish_command(), NO_CLAIM)
            .await?;
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// planning-claim-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac3_an_update_only_plan_finishes_and_a_plan_without_changes_is_refused() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.held_t0().await?;
        p.refuse_command("no holder changes", finish_command(), NO_TASK_CHANGES)
            .await?;
        for (transport, claim) in p.planning_claims("criterion:a").await? {
            ensure!(claim["actor"] == "agent", "{transport}: {claim}");
        }
        let verification = p.read("criterion:a").await?["process"]["verification"].clone();
        p.update_t0().await?;
        p.finish(mcp).await?;
        for (transport, claim) in p.planning_claims("criterion:a").await? {
            ensure!(claim.is_null(), "mcp={mcp} {transport}: {claim}");
        }
        ensure!(p.read("criterion:a").await?["process"]["verification"] == verification);
        for (transport, t0) in p.object(&w.token, "work:t0").await? {
            ensure!(
                t0["process"]["status"] == "in_progress"
                    && t0["process"]["assignee"] == "worker"
                    && t0["document"]["data"]["contract"] == "Implement the updated plan",
                "{transport}: {t0}"
            );
        }
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// planning-claim-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac3_a_plan_that_depends_on_an_archived_criterion_is_refused() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    let revision = p.prerequisite("Deliver the prerequisite").await?;
    p.ready().await?;
    p.claim("agent", "criterion:a").await?;
    p.plan_task(
        "work:t1",
        "Implement this criterion",
        json!([{"target":"criterion:c","relation":"depends_on","revision":revision,"blocking":true}]),
    )
    .await?;
    // The owner archives C in spec:two after P links the new Task to it.
    p.write("owner", json!({"op":"archive_object","target":"criterion:c","revision":revision,"reason":"Drop the prerequisite"})).await?;
    ensure!(p.read("criterion:c").await?["document"]["present"] == false);
    p.refuse_command(
        "archived dependency target",
        finish_command(),
        "planning dependency target is missing",
    )
    .await?;
    p.unpublished(&["work:t1"]).await?;
    store::check_journal(&p.pool).await?;
    Ok(())
}

// planning-claim-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac4_a_holder_release_retires_only_the_new_tasks_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let fields = |t: &Value| {
        json!([
            t["process"]["status"],
            t["process"]["assignee"],
            t["current_revision"],
            t["document"]
        ])
    };
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.held_t0().await?;
        for task in ["work:t1", "work:t2"] {
            p.plan_task(task, "Implement this criterion", json!([]))
                .await?;
        }
        p.update_t0().await?;
        let t0 = p.object(&w.token, "work:t0").await?;
        let history = p.history("work:t0").await?;
        for (transport, t) in &t0 {
            ensure!(
                t["process"]["status"] == "in_progress"
                    && t["process"]["assignee"] == "worker"
                    && t["document"]["actor"] == "agent"
                    && history.len() as u64 == t["current_revision"].as_u64().unwrap(),
                "{transport}: {t}"
            );
        }

        p.send(mcp, &release_command("Plan withdrawn"))
            .await?
            .map_err(anyhow::Error::msg)?;
        for (transport, claim) in p.planning_claims("criterion:a").await? {
            ensure!(claim.is_null(), "mcp={mcp} {transport}: {claim}");
        }
        for task in ["work:t1", "work:t2"] {
            for (transport, t) in p.object(&w.token, task).await? {
                ensure!(
                    t["process"]["status"] == "retired"
                        && t["process"]["retired"] == "Plan withdrawn"
                        && t["process"]["assignee"].is_null()
                        && t["document"]["present"] == true,
                    "{transport} {task}: {t}"
                );
            }
        }
        for ((transport, old), (_, new)) in t0.iter().zip(p.object(&w.token, "work:t0").await?) {
            ensure!(fields(old) == fields(&new), "{transport}: {old} {new}");
        }
        ensure!(p.history("work:t0").await? == history, "T0 history changed");
        let saved = p.saved().await?;
        let event = saved["events"]
            .as_array()
            .unwrap()
            .iter()
            .max_by_key(|e| e["position"].as_i64())
            .cloned()
            .unwrap_or_default();
        ensure!(
            event["actor"] == "agent"
                && event["command"]["op"] == "release_planning"
                && event["command"]["reason"] == "Plan withdrawn",
            "{event}"
        );
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

// planning-claim-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac4_an_empty_release_succeeds_and_each_release_refusal_changes_nothing() -> Result<()> {
    let w = world().await?;
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.ready().await?;
        p.claim("agent", "criterion:a").await?;
        p.refuse_command_as(
            &w.other,
            "Q releases A",
            release_command("Plan withdrawn"),
            "workspace owner required",
        )
        .await?;
        p.refuse_command("blank reason", release_command(" "), EMPTY_TEXT)
            .await?;
        let objects = p.addresses().await?;
        p.send(mcp, &release_command("Plan withdrawn"))
            .await?
            .map_err(anyhow::Error::msg)?;
        for (transport, claim) in p.planning_claims("criterion:a").await? {
            ensure!(claim.is_null(), "mcp={mcp} {transport}: {claim}");
        }
        ensure!(p.addresses().await? == objects, "release changed objects");
        p.refuse_command(
            "no active claim",
            release_command("Plan withdrawn"),
            NO_CLAIM,
        )
        .await?;
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}

/// One MCP joppa_batch operation for each criterion.
fn per_criterion(command: impl Fn(&str) -> Value) -> Value {
    json!(["criterion:a", "criterion:b"].map(|ac| json!({"command":command(ac)})))
}

// planning-claim-ac8
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac8_a_planner_claims_and_finishes_two_criteria_of_a_req_in_one_mcp_batch() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.ready().await?;
    let response = planning_batch_in(&p, claim_batch(&p).await?).await?;
    ensure!(response["result"]["isError"] != true, "{response}");
    for ac in ["criterion:a", "criterion:b"] {
        for (transport, claim) in p.planning_claims(ac).await? {
            ensure!(claim["actor"] == "agent", "{transport} {ac}: {claim}");
        }
    }
    for (id, ac) in [("a1", "criterion:a"), ("b1", "criterion:b")] {
        p.write("agent", json!({"op":"save_object","kind":"work","id":id,"revision":0,"parent":ac,"data":{"title":id,"owner":"owner","contract":"Implement this criterion","purpose":"implementation"},"reason":"Plan the work"})).await?;
    }
    let finish =
        per_criterion(|ac| json!({"op":"finish_planning","ac":ac,"reason":"Plan complete"}));
    let response = planning_batch_in(&p, finish).await?;
    ensure!(response["result"]["isError"] != true, "{response}");
    for ac in ["criterion:a", "criterion:b"] {
        for (transport, claim) in p.planning_claims(ac).await? {
            ensure!(claim.is_null(), "{transport} {ac}: {claim}");
        }
    }
    for task in ["work:a1", "work:b1"] {
        for (transport, t) in p.entry(&w.token, task).await? {
            ensure!(t["claimable"] == true, "{transport} {task}: {t}");
        }
    }
    store::check_journal(&p.pool).await?;

    // Q holds B: the whole claim batch of P is refused.
    let p = planning(&w).await?;
    p.ready().await?;
    p.claim("other", "criterion:b").await?;
    let claims = claim_batch(&p).await?;
    let before = p.saved().await?;
    let response = planning_batch_in(&p, claims).await?;
    ensure!(
        response["result"]["isError"] == true
            && response["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|e| e.ends_with("criterion is already claimed for planning")),
        "{response}"
    );
    let after = p.saved().await?;
    ensure!(
        after == before && after["position"] == before["position"],
        "the refused batch changed saved state"
    );
    for (transport, claim) in p.planning_claims("criterion:a").await? {
        ensure!(claim.is_null(), "{transport}: {claim}");
    }
    store::check_journal(&p.pool).await?;
    Ok(())
}

/// claim_planning for A and B at their current revisions.
async fn claim_batch(p: &Planning<'_>) -> Result<Value> {
    let (a, b) = (
        p.revision("criterion:a").await?,
        p.revision("criterion:b").await?,
    );
    Ok(per_criterion(|ac| {
        let revision = if ac == "criterion:a" { a } else { b };
        json!({"op":"claim_planning","ac":ac,"revision":revision})
    }))
}

/// P sends one MCP joppa_batch in the planning workspace.
async fn planning_batch_in(p: &Planning<'_>, operations: Value) -> Result<Value> {
    p.mcp(
        "joppa_batch",
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"operations":operations}),
    )
    .await
}

/// The failing check that red sibling B lists in the frozen fixtures.
const REJECTS: &str = "tests/auth/login.test.ts > login > rejects";

/// check-list-frozen-ac3: area G holds R with criterion A and red sibling B; area H is
/// another parent. A lists A and B, B lists a failing check, both are assessed ready,
/// and one record makes A green. With `plan`, the agent claims A for planning
/// and saves draft Task work:t before the run.
async fn frozen(w: &World, plan: bool) -> Result<Planning<'_>> {
    let p = Planning {
        w,
        pool: store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?,
        ws: format!("http-frozen-{}", uuid::Uuid::new_v4().simple()),
    };
    store::execute_input(
        &p.pool,
        &p.ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Every change resets"}),
    )
    .await?;
    p.write(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(&p.pool, &p.ws, "owner", None).await?;
    p.write("owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure the fixture tree","types":{
        "area":{"title":"Area","role":"group","status":"children","root":true,"parents":["area"]},
        "spec":{"title":"Specification","role":"requirement","status":"children","parents":["area"]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"]},
        "signal":{"title":"Signal","role":"document","status":"none","root":true,"parents":[],"inbox":true}
    }})).await?;
    for key in ["area:g", "area:h"] {
        p.save(
            key,
            None,
            json!({"title":key,"body":"Group scope","owner":"owner"}),
            json!([]),
        )
        .await?;
    }
    p.save(
        "spec:r",
        Some("area:g"),
        json!({"title":"R","body":"Implement the rule","owner":"owner"}),
        json!([]),
    )
    .await?;
    for (ac, tests) in [("criterion:a", vec![A, B]), ("criterion:b", vec![REJECTS])] {
        p.save(
            ac,
            Some("spec:r"),
            json!({"title":ac,"owner":"owner"}),
            json!([]),
        )
        .await?;
        p.write(
            "agent",
            json!({"op":"set_ac_tests","target":ac,"tests":tests,"reason":"Bind the tests"}),
        )
        .await?;
    }
    let ready = json!({"ready":true,"reason":"Known scope"});
    p.confirm(
        "spec:r",
        &[("criterion:a", ready.clone()), ("criterion:b", ready)],
    )
    .await?;
    if plan {
        p.claim("agent", "criterion:a").await?;
        p.write("agent", json!({"op":"save_object","kind":"work","id":"t","revision":0,"parent":"criterion:a","data":{"title":"T","owner":"owner","contract":"Implement this criterion","purpose":"implementation"},"links":[],"reason":"Plan the criterion"})).await?;
    }
    p.record("c1c1c1c", frozen_run()).await?;
    let (a, b) = (
        p.ac_tests("criterion:a").await?,
        p.ac_tests("criterion:b").await?,
    );
    ensure!(
        a["status"] == "green" && b["status"] == "red",
        "A must start green and B red: {a} {b}"
    );
    Ok(p)
}

/// A record that passes both tests of A and fails the test of B.
fn frozen_run() -> Value {
    outcomes(&[("opens", "pass"), ("refuses", "pass"), ("rejects", "fail")])
}

impl Planning<'_> {
    async fn ac_tests(&self, ac: &str) -> Result<Value> {
        self.get(format!("/tests?ac={ac}")).await
    }
    async fn assessment(&self, ac: &str) -> Result<Value> {
        Ok(self.get(format!("/objects?address={ac}")).await?["process"]["assessment"].clone())
    }
    /// `token` posts `command` to the HTTP command endpoint with a new request ID.
    async fn post_command(&self, token: &str, command: Value) -> Result<()> {
        let root = self.w.base.split("/api/").next().unwrap();
        let reply = self
            .w
            .http
            .post(format!("{root}/api/workspaces/{}/commands", self.ws))
            .bearer_auth(token)
            .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}))
            .send()
            .await?;
        let code = reply.status();
        ensure!(code.is_success(), "{code} {}", reply.text().await?);
        Ok(())
    }
    /// Owner S saves R over HTTP with `parent`, `title` and `links`.
    async fn save_r(&self, parent: &str, title: &str, links: Value) -> Result<()> {
        let revision = self.revision("spec:r").await?;
        self.post_command(&self.w.owner, json!({"op":"save_object","kind":"spec","id":"r","revision":revision,"parent":parent,"data":{"title":title,"body":"Implement the rule","owner":"owner"},"links":links,"reason":"Change R"})).await
    }
    /// Agent bot saves the identical test list of A over HTTP.
    async fn resave_a(&self) -> Result<()> {
        self.post_command(&self.w.token, json!({"op":"set_ac_tests","target":"criterion:a","tests":[A,B],"reason":"Save the list again"})).await
    }
    /// Owner S saves inbox signal s under R over HTTP.
    async fn signal(&self) -> Result<()> {
        self.post_command(&self.w.owner, json!({"op":"save_object","kind":"signal","id":"s","revision":0,"parent":null,"data":{"title":"Provider change","owner":"owner","body":"New permission rules"},"links":[{"target":"spec:r","relation":"related"}],"reason":"Record the signal"})).await
    }
    /// A is grey with the test list of `before` and has no readiness assessment.
    async fn reset(&self, before: &Value) -> Result<()> {
        let s = self.ac_tests("criterion:a").await?;
        ensure!(
            s["status"] == "grey" && addresses(&s) == addresses(before),
            "A after the change: {s}"
        );
        let readiness = self.assessment("criterion:a").await?;
        ensure!(
            readiness["state"] == "unassessed",
            "A readiness: {readiness}"
        );
        Ok(())
    }
    /// A is still green with the test list of `before` and the readiness `assessment`.
    async fn kept(&self, before: &Value, assessment: &Value) -> Result<()> {
        let s = self.ac_tests("criterion:a").await?;
        ensure!(
            s["status"] == "green" && addresses(&s) == addresses(before),
            "A after the change: {s}"
        );
        ensure!(
            self.assessment("criterion:a").await? == *assessment,
            "the readiness of A changed"
        );
        Ok(())
    }
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f1_a_new_title_of_r_makes_a_grey_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    p.save_r("area:g", "R renamed", json!([])).await?;
    p.reset(&before).await?;
    let mcp = p
        .mcp_read("joppa_tests", json!({"ac":"criterion:a"}))
        .await?;
    ensure!(mcp["status"] == "grey", "MCP joppa_tests: {mcp}");
    Ok(())
}

// ready-is-the-only-gate-ac2
// R is requirement:key at revision 1, and A is its green ready criterion ac:key-rights.
// Agent bot saves a new text of R. Revision 1 of R keeps the old text.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_save_of_r_makes_revision_2_greys_a_and_resets_its_readiness() -> Result<()> {
    let w = world().await?;
    let workspace = w.base.rsplit('/').next().unwrap().to_owned();
    // Bot reads joppa_objects over HTTP and MCP; both give the same reply.
    let read = |query: &'static str, mut arguments: Value| {
        let w = &w;
        arguments["workspace"] = json!(workspace);
        async move {
            let http: Value = w
                .http
                .get(format!("{}/objects?{query}", w.base))
                .bearer_auth(&w.token)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let mcp = mcp_read(w, &w.token, "joppa_objects", arguments).await?;
            ensure!(http == mcp, "joppa_objects {query}: HTTP {http}\nMCP {mcp}");
            anyhow::Ok(http)
        }
    };
    let tests = || async {
        let mcp = mcp_read(
            &w,
            &w.token,
            "joppa_tests",
            json!({"workspace":workspace,"ac":"ac:key-rights"}),
        )
        .await?;
        let http = w.status().await?;
        ensure!(http == mcp, "joppa_tests: HTTP {http}\nMCP {mcp}");
        anyhow::Ok(http)
    };
    ready_for_planning(&w).await?;
    w.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    // Controls: R is at revision 1, and A is green and ready for work.
    let r = read(
        "address=requirement:key",
        json!({"address":"requirement:key"}),
    )
    .await?;
    ensure!(r["current_revision"] == 1, "R before the save: {r}");
    ensure!(tests().await?["status"] == "green", "A must start green");
    let a = read("address=ac:key-rights", json!({"address":"ac:key-rights"})).await?;
    ensure!(
        a["process"]["assessment"]["state"] == "ready",
        "A before the save: {a}"
    );
    let mut data = r["document"]["data"].clone();
    data["body"] = json!("Changed scope");
    let (code, reply) = post(&w, &w.token, "/commands", json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":
        {"op":"save_object","kind":"requirement","id":"key","revision":1,"parent":r["document"]["parent"],
         "data":data,"links":[],"reason":"Change R"}})).await?;
    ensure!(code == 200, "bot saves R: {code} {reply}");
    let r = read(
        "address=requirement:key",
        json!({"address":"requirement:key"}),
    )
    .await?;
    ensure!(
        r["current_revision"] == 2
            && r["document"]["actor"] == "agent"
            && r["document"]["data"]["body"] == "Changed scope",
        "R after the save: {r}"
    );
    let status = tests().await?;
    ensure!(status["status"] == "grey", "A after the save: {status}");
    let a = read("address=ac:key-rights", json!({"address":"ac:key-rights"})).await?;
    ensure!(
        a["process"]["assessment"]["state"] == "unassessed"
            && a["process"]["readiness_blocker"]["state"] == "unassessed",
        "A after the save: {a}"
    );
    let old = read(
        "address=requirement:key&revision=1",
        json!({"address":"requirement:key","revision":1}),
    )
    .await?;
    ensure!(
        old["historical"] == true
            && old["document"]["revision"] == 1
            && old["document"]["data"]["body"] == "Scope",
        "R at revision 1: {old}"
    );
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f2_a_new_link_of_r_makes_a_grey() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    p.save_r(
        "area:g",
        "R",
        json!([{"target":"area:h","relation":"related"}]),
    )
    .await?;
    p.reset(&before).await
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f3_a_move_of_r_makes_a_grey() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    p.save_r("area:h", "R", json!([])).await?;
    p.reset(&before).await
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f4_a_comment_on_r_keeps_a_green() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let (before, assessment) = (
        p.ac_tests("criterion:a").await?,
        p.assessment("criterion:a").await?,
    );
    p.post_command(
        &w.owner,
        json!({"op":"note","target":"spec:r","id":"context","text":"Keep this context"}),
    )
    .await?;
    p.kept(&before, &assessment).await
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f5_an_identical_list_save_makes_a_grey_and_unassessed() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    let r = p.read("spec:r").await?;
    ensure!(
        p.assessment("criterion:a").await?["state"] == "ready",
        "A must start ready"
    );
    p.resave_a().await?;
    p.reset(&before).await?;
    let after = p.read("spec:r").await?;
    ensure!(
        after["current_revision"] == r["current_revision"]
            && after["process"]["revision"] == r["process"]["revision"],
        "R has a new revision: {after}"
    );
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f6_an_archived_red_sibling_makes_a_and_r_grey() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    let r = p.get("/objects?address=spec:r".into()).await?;
    ensure!(r["verification_summary"]["passed"] == 1, "{r}");
    let revision = p.revision("criterion:b").await?;
    p.post_command(&w.owner, json!({"op":"archive_object","target":"criterion:b","revision":revision,"reason":"Drop the red criterion"})).await?;
    p.reset(&before).await?;
    let r = p
        .mcp_read("joppa_objects", json!({"address":"spec:r"}))
        .await?;
    ensure!(
        r["verification_summary"] == json!({"total":1,"passed":0,"failed":0}),
        "MCP joppa_objects R: {r}"
    );
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f6_control_an_archived_inbox_signal_keeps_a_green() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    p.signal().await?;
    let (before, assessment) = (
        p.ac_tests("criterion:a").await?,
        p.assessment("criterion:a").await?,
    );
    p.post_command(
        &w.owner,
        json!({"op":"archive_object","target":"signal:s","revision":1,"reason":"Drop the signal"}),
    )
    .await?;
    p.kept(&before, &assessment).await
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f7_a_pm_marks_grey_a_ready_and_a_stays_grey() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    p.resave_a().await?;
    p.reset(&before).await?;
    let revision = p.read("spec:r").await?["process"]["revision"].clone();
    p.write("owner", json!({"op":"assess","req":"spec:r","revision":revision,"ac":"criterion:a","assessment":{"ready":true,"reason":"Known scope"}})).await?;
    let readiness = p.assessment("criterion:a").await?;
    let s = p.ac_tests("criterion:a").await?;
    ensure!(
        readiness["state"] == "ready" && s["status"] == "grey",
        "{readiness} {s}"
    );
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f8_only_a_record_after_the_change_makes_a_green() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let before = p.ac_tests("criterion:a").await?;
    let b = (
        p.ac_tests("criterion:b").await?,
        p.assessment("criterion:b").await?,
    );
    p.resave_a().await?;
    // Control: the record from before the change does not count.
    p.reset(&before).await?;
    // Control: a record after the change that misses a listed test stays grey.
    p.record("c2c2c2c", outcomes(&[("opens", "pass")])).await?;
    let s = p.ac_tests("criterion:a").await?;
    ensure!(s["status"] == "grey", "after records: {s}");
    p.record("c4c4c4c", frozen_run()).await?;
    let s = p.ac_tests("criterion:a").await?;
    ensure!(s["status"] == "green", "after the record: {s}");
    let mcp = p
        .mcp_read("joppa_tests", json!({"ac":"criterion:a"}))
        .await?;
    ensure!(mcp["status"] == "green", "MCP joppa_tests: {mcp}");
    let after = (
        p.ac_tests("criterion:b").await?,
        p.assessment("criterion:b").await?,
    );
    ensure!(
        after.0["status"] == b.0["status"] && after.1 == b.1,
        "sibling B changed: {after:?}"
    );
    Ok(())
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f9_finish_planning_under_green_a_keeps_a_green() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, true).await?;
    let (before, assessment) = (
        p.ac_tests("criterion:a").await?,
        p.assessment("criterion:a").await?,
    );
    p.post_command(
        &w.token,
        json!({"op":"finish_planning","ac":"criterion:a","reason":"Plan complete"}),
    )
    .await?;
    let a = p.read("criterion:a").await?;
    ensure!(a["process"]["planning_claim"].is_null(), "{a}");
    p.kept(&before, &assessment).await
}

// check-list-frozen-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn frozen_f10_an_inbox_signal_under_r_keeps_a_green() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    let (before, assessment) = (
        p.ac_tests("criterion:a").await?,
        p.assessment("criterion:a").await?,
    );
    p.signal().await?;
    p.kept(&before, &assessment).await
}

// signal-becomes-spec-ac3
// Member alex archives one signal under R over HTTP and one over MCP. A and R keep
// their status, readiness and verification summary. Control: a new title of R makes A grey.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_archive_of_a_signal_under_r_keeps_a_green_and_ready() -> Result<()> {
    let w = world().await?;
    let p = frozen(&w, false).await?;
    p.write("owner", json!({"op":"member","id":"alex","role":"human"}))
        .await?;
    for id in ["http-n", "mcp-n"] {
        p.post_command(&w.owner, json!({"op":"save_object","kind":"signal","id":id,"revision":0,"parent":null,"data":{"title":"Provider change","owner":"owner","body":"New permission rules"},"links":[{"target":"spec:r","relation":"related"}],"reason":"Record the signal"})).await?;
    }
    let state = async || -> Result<Value> {
        let a = p
            .mcp_read("joppa_objects", json!({"address":"criterion:a"}))
            .await?;
        let r = p
            .mcp_read("joppa_objects", json!({"address":"spec:r"}))
            .await?;
        Ok(json!({
            "status": p.ac_tests("criterion:a").await?["status"],
            "assessment": a["process"]["assessment"],
            "summary": r["verification_summary"],
        }))
    };
    let before = state().await?;
    ensure!(
        before["status"] == "green" && before["assessment"]["state"] == "ready",
        "A must start green and ready: {before}"
    );
    for (mcp, signal) in [(false, "signal:http-n"), (true, "signal:mcp-n")] {
        let archive =
            json!({"op":"archive_object","target":signal,"revision":1,"reason":"Needs no work"});
        let reply = p.send_as(&w.alex, mcp, &archive).await?;
        ensure!(reply.is_ok(), "mcp={mcp}: {reply:?}");
        let after = state().await?;
        ensure!(after == before, "mcp={mcp}: {before} became {after}");
        let n = p
            .mcp_read("joppa_objects", json!({"address":signal}))
            .await?;
        ensure!(n["document"]["present"] == false, "mcp={mcp}: {n}");
    }
    // Control: the fixture detects a reset.
    let tests = p.ac_tests("criterion:a").await?;
    p.save_r("area:g", "R renamed", json!([])).await?;
    p.reset(&tests).await
}

// check-list-frozen-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac1_a_text_change_of_r_greys_its_criteria_and_keeps_the_sibling_requirement_green()
-> Result<()> {
    let w = world().await?;
    let p = Planning {
        w: &w,
        pool: store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?,
        ws: format!("http-frozen-ac1-{}", uuid::Uuid::new_v4().simple()),
    };
    store::execute_input(
        &p.pool,
        &p.ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"A requirement change resets"}),
    )
    .await?;
    p.write(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(&p.pool, &p.ws, "owner", None).await?;
    p.write("owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure the fixture tree","types":{
        "area":{"title":"Area","role":"group","status":"children","root":true,"parents":["area"]},
        "spec":{"title":"Specification","role":"requirement","status":"children","parents":["area"]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]}
    }})).await?;
    // P holds R with criteria A and B, and R2 with criterion C.
    p.save(
        "area:p",
        None,
        json!({"title":"P","body":"Group scope","owner":"owner"}),
        json!([]),
    )
    .await?;
    for spec in ["spec:r", "spec:r2"] {
        p.save(
            spec,
            Some("area:p"),
            json!({"title":spec,"body":"Implement the rule","owner":"owner"}),
            json!([]),
        )
        .await?;
    }
    let criteria = [
        ("criterion:a", "spec:r", A),
        ("criterion:b", "spec:r", B),
        ("criterion:c", "spec:r2", REJECTS),
    ];
    for (ac, spec, test) in criteria {
        p.save(
            ac,
            Some(spec),
            json!({"title":ac,"owner":"owner"}),
            json!([]),
        )
        .await?;
        p.write(
            "agent",
            json!({"op":"set_ac_tests","target":ac,"tests":[test],"reason":"Bind the tests"}),
        )
        .await?;
    }
    let ready = json!({"ready":true,"reason":"Known scope"});
    p.confirm(
        "spec:r",
        &[
            ("criterion:a", ready.clone()),
            ("criterion:b", ready.clone()),
        ],
    )
    .await?;
    p.confirm("spec:r2", &[("criterion:c", ready)]).await?;
    p.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass"), ("rejects", "pass")]),
    )
    .await?;
    let mut before = std::collections::BTreeMap::new();
    for (ac, _, _) in criteria {
        let s = p.ac_tests(ac).await?;
        let mcp = p.mcp_read("joppa_tests", json!({"ac":ac})).await?;
        let readiness = p.assessment(ac).await?;
        ensure!(
            s["status"] == "green" && mcp["status"] == "green" && readiness["state"] == "ready",
            "{ac} before the change: {s} {mcp} {readiness}"
        );
        before.insert(ac, (s, readiness));
    }

    // Owner S changes only the text of R through the ordinary save.
    p.owner_save(
        "spec:r",
        Some("area:p"),
        json!({"title":"spec:r","body":"Changed rule","owner":"owner"}),
    )
    .await?;
    for ac in ["criterion:a", "criterion:b"] {
        let s = p.ac_tests(ac).await?;
        let mcp = p.mcp_read("joppa_tests", json!({"ac":ac})).await?;
        ensure!(
            s["status"] == "grey"
                && mcp["status"] == "grey"
                && addresses(&s) == addresses(&before[ac].0),
            "{ac} after the change: {s} {mcp}"
        );
        let readiness = p.assessment(ac).await?;
        ensure!(
            readiness["state"] == "unassessed",
            "{ac} readiness: {readiness}"
        );
    }
    let c = p.ac_tests("criterion:c").await?;
    let mcp = p
        .mcp_read("joppa_tests", json!({"ac":"criterion:c"}))
        .await?;
    ensure!(
        c["status"] == "green" && mcp["status"] == "green",
        "C after the change: {c} {mcp}"
    );
    ensure!(
        p.assessment("criterion:c").await? == before["criterion:c"].1,
        "the readiness of C changed"
    );
    let r = p
        .mcp_read("joppa_objects", json!({"address":"spec:r"}))
        .await?;
    ensure!(
        r["verification_summary"]["passed"] == 0,
        "MCP joppa_objects R: {r}"
    );
    Ok(())
}

/// dependencies-before-priority: capability C holds specification B with criterion B1.
/// Specification A sits under `a_parent` with `a_criteria`; capability D exists for AC7.
/// Each criterion is assessed ready and has no test report yet, so each one is grey.
async fn ordering<'a>(w: &'a World, a_parent: &str, a_criteria: &[&str]) -> Result<Planning<'a>> {
    let p = Planning {
        w,
        pool: store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?,
        ws: format!("http-ordering-{}", uuid::Uuid::new_v4().simple()),
    };
    store::execute_input(
        &p.pool,
        &p.ws,
        "owner",
        "workspace",
        &json!({"op":"workspace","title":"Ordering links"}),
    )
    .await?;
    p.write(
        "owner",
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(&p.pool, &p.ws, "owner", None).await?;
    p.write("owner", json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure ordered work","types":{
        "cap":{"title":"Capability","role":"group","status":"children","root":true,"parents":[]},
        "spec":{"title":"Specification","role":"requirement","status":"children","parents":["cap"]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    for (key, parent) in [
        ("cap:c", None),
        ("cap:d", None),
        ("spec:a", Some(a_parent)),
        ("spec:b", Some("cap:c")),
    ] {
        p.save(
            key,
            parent,
            json!({"title":key,"body":"Deliver the rule","owner":"owner"}),
            json!([]),
        )
        .await?;
    }
    for (key, parent) in a_criteria
        .iter()
        .map(|key| (*key, "spec:a"))
        .chain([("criterion:b1", "spec:b")])
    {
        p.save(
            key,
            Some(parent),
            json!({"title":key,"owner":"owner"}),
            json!([]),
        )
        .await?;
    }
    let ready = json!({"ready":true,"reason":"Known scope"});
    let a: Vec<(&str, Value)> = a_criteria.iter().map(|key| (*key, ready.clone())).collect();
    p.confirm("spec:a", &a).await?;
    p.confirm("spec:b", &[("criterion:b1", ready)]).await?;
    Ok(p)
}

/// A blocking ordering link without a revision.
fn ordering_link(target: &str) -> Value {
    json!([{"target":target,"relation":"depends_on","blocking":true}])
}

impl Planning<'_> {
    /// Bot reads joppa_objects over HTTP and MCP; both give the same reply.
    async fn objects_both(&self, query: &str, arguments: Value) -> Result<Value> {
        let http = self.get(format!("/objects?{query}")).await?;
        let mcp = self.mcp_read("joppa_objects", arguments).await?;
        ensure!(http == mcp, "joppa_objects {query}: HTTP {http}\nMCP {mcp}");
        Ok(http)
    }
    async fn object_both(&self, address: &str) -> Result<Value> {
        self.objects_both(&format!("address={address}"), json!({"address":address}))
            .await
    }
    /// The addresses of work_filter=unstarted over HTTP and MCP.
    async fn unstarted(&self) -> Result<Vec<String>> {
        let page = self
            .objects_both(
                "work_filter=unstarted&limit=100",
                json!({"work_filter":"unstarted","limit":100}),
            )
            .await?;
        Ok(page["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["address"].as_str().map(str::to_owned))
            .collect())
    }
    /// Bot reads joppa_tasks over HTTP and MCP; both give the same reply.
    async fn tasks_both(&self, view: &str) -> Result<Vec<Value>> {
        let http = self.get(format!("/tasks?view={view}&limit=100")).await?;
        let mcp = self
            .mcp_read("joppa_tasks", json!({"view":view,"limit":100}))
            .await?;
        ensure!(http == mcp, "joppa_tasks {view}: HTTP {http}\nMCP {mcp}");
        Ok(http["tasks"].as_array().cloned().unwrap_or_default())
    }
    async fn status(&self, ac: &str) -> Result<Value> {
        self.get(format!("/tests?ac={ac}")).await
    }
    /// Member P (the owner) sends one command over HTTP.
    async fn owner_command(&self, command: Value) -> Result<Value> {
        let root = self.w.base.split("/api/").next().unwrap();
        let reply = self
            .w
            .http
            .post(format!("{root}/api/workspaces/{}/commands", self.ws))
            .bearer_auth(&self.w.owner)
            .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}))
            .send()
            .await?;
        let code = reply.status().as_u16();
        let body: Value = reply.json().await.unwrap_or(Value::Null);
        ensure!(code == 200, "{command}: {code} {body}");
        Ok(body)
    }
    /// P saves the current document of `key` with `links` over HTTP.
    async fn link(&self, key: &str, links: Value) -> Result<Value> {
        let (kind, id) = key.split_once(':').unwrap();
        let object = self.read(key).await?;
        let document = &object["document"];
        self.owner_command(json!({"op":"save_object","kind":kind,"id":id,"revision":object["current_revision"],
            "parent":document["parent"],"data":document["data"],"links":links,"reason":"Order the work"}))
            .await
    }
    /// Readiness without the revision number of the specification.
    async fn readiness(&self, ac: &str) -> Result<Value> {
        let a = &self.read(ac).await?["process"]["assessment"];
        Ok(json!([
            a["state"],
            a["reason"],
            a["recipient"],
            a["decision"]
        ]))
    }
}

/// A waiting criterion reads work_ready=false and names `target` as its readiness blocker.
fn waits_for(object: &Value, target: &str) -> bool {
    object["work_ready"] == false
        && object["process"]["readiness_blocker"]["target"] == target
        && object["waits_for"]["address"] == target
        && object["process"]["readiness_blocker"]["reason"]
            .as_str()
            .is_some_and(|r| r.contains(target))
}

// dependencies-before-priority-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac1_a_criterion_waits_until_its_dependency_is_green_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1"]).await?;
    p.link("spec:b", ordering_link("spec:a")).await?;
    let b = p.object_both("spec:b").await?;
    ensure!(
        b["document"]["links"]
            == json!([{"target":"spec:a","relation":"depends_on","blocking":true}]),
        "the link stores no revision: {b}"
    );
    // Controls: A1 is ready and grey. The priority control is pending: set_priority is absent.
    let a1 = p.object_both("criterion:a1").await?;
    ensure!(
        a1["process"]["assessment"]["state"] == "ready" && a1["work_ready"] == true,
        "{a1}"
    );
    ensure!(p.status("criterion:a1").await?["status"] == "grey");
    let listed = p.unstarted().await?;
    ensure!(
        listed.contains(&"criterion:a1".to_owned()) && !listed.contains(&"criterion:b1".to_owned()),
        "{listed:?}"
    );
    let b1 = p.object_both("criterion:b1").await?;
    ensure!(
        b1["process"]["assessment"]["state"] == "ready" && waits_for(&b1, "spec:a"),
        "{b1}"
    );
    // A planning claim on B1 names A and adds no journal entry.
    p.refuse(
        "planning B1 before A is green",
        "criterion:b1",
        b1["current_revision"].as_u64().unwrap(),
        &joppa::dependencies::reason("spec:a"),
    )
    .await?;

    // A record passes every listed test of A1.
    p.write(
        "agent",
        json!({"op":"set_ac_tests","target":"criterion:a1","tests":[A],"reason":"Bind the test"}),
    )
    .await?;
    let report = outcomes(&[("opens", "pass"), ("refuses", "pass")]);
    p.record("c1c1c1c", report.clone()).await?;
    ensure!(p.status("criterion:a1").await?["status"] == "green");
    p.owner_command(
        json!({"op":"note","target":"cap:c","id":"unrelated","text":"An unrelated remark"}),
    )
    .await?;
    for repeat in [false, true] {
        if repeat {
            // The same record again is a new receipt of it.
            p.record("c1c1c1c", report.clone()).await?;
        }
        ensure!(
            p.unstarted().await?.contains(&"criterion:b1".to_owned()),
            "repeat={repeat}"
        );
        let b1 = p.object_both("criterion:b1").await?;
        ensure!(
            b1["work_ready"] == true
                && b1["process"]["readiness_blocker"].is_null()
                && b1.get("waits_for").is_none(),
            "repeat={repeat}: {b1}"
        );
    }

    // A Task pin to a changed Task contract still holds its dependent after a rebuild.
    let contract = |title: &str| json!({"title":title,"owner":"owner","contract":"Implement the agreed mechanism","purpose":"implementation"});
    p.save(
        "work:source",
        Some("criterion:a1"),
        contract("Source"),
        json!([]),
    )
    .await?;
    let pin = p.revision("work:source").await?;
    p.save(
        "work:dependent",
        Some("criterion:a1"),
        contract("Dependent"),
        json!([{"target":"work:source","relation":"depends_on","revision":pin,"blocking":true}]),
    )
    .await?;
    p.write("owner", json!({"op":"amend_task","id":"work:source","revision":p.revision("work:source").await?,"title":"Changed source","contract":"Implement the agreed mechanism","projects":[],"reason":"Change the source"})).await?;
    let unclaimable = |tasks: Vec<Value>| !tasks.iter().any(|t| t["address"] == "work:dependent");
    ensure!(unclaimable(p.tasks_both("available").await?));
    ensure!(p.read("work:dependent").await?["review_required"] == true);
    for table in ["state_entries", "board_cache"] {
        p.pool
            .get()
            .await?
            .execute(
                &format!("DELETE FROM joppa.{table} WHERE workspace=$1"),
                &[&p.ws],
            )
            .await?;
    }
    store::migrate(&p.pool).await?;
    ensure!(unclaimable(p.tasks_both("available").await?));
    let dependent = p.read("work:dependent").await?;
    ensure!(
        dependent["review_required"] == true
            && dependent["document"]["links"][0]["revision"] == pin,
        "{dependent}"
    );
    ensure!(p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    store::check_journal(&p.pool).await?;
    Ok(())
}

// dependencies-before-priority-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac2_a_task_under_waiting_work_cannot_be_claimed_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1"]).await?;
    p.link("spec:b", ordering_link("spec:a")).await?;
    p.save("work:t", Some("criterion:b1"), json!({"title":"T","owner":"owner","contract":"Implement this criterion","purpose":"implementation"}), json!([]))
        .await?;
    ensure!(p.tasks_both("mine").await?.is_empty());
    let reason = joppa::dependencies::reason("spec:a");
    let revision = p.revision("work:t").await?;
    p.refuse_command(
        "claim_task on T",
        json!({"op":"claim_task","id":"work:t","revision":revision}),
        &reason,
    )
    .await?;
    p.refuse_command(
        "claim_next_task with only T",
        json!({"op":"claim_next_task"}),
        &reason,
    )
    .await?;
    let t = p.object_both("work:t").await?;
    ensure!(
        t["process"]["assignee"].is_null() && t["process"]["status"] == "open",
        "{t}"
    );
    let listed = |tasks: &[Value]| tasks.iter().find(|t| t["address"] == "work:t").cloned();
    ensure!(listed(&p.tasks_both("available").await?).is_none());
    for view in ["all", "waiting"] {
        let t = listed(&p.tasks_both(view).await?);
        ensure!(
            t.as_ref()
                .is_some_and(|t| t["claimable"] == false && t["waiting_reason"] == reason.as_str()),
            "view={view}: {t:?}"
        );
    }
    // Release: a record makes every criterion of A green.
    p.write(
        "agent",
        json!({"op":"set_ac_tests","target":"criterion:a1","tests":[A],"reason":"Bind the test"}),
    )
    .await?;
    p.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let t = listed(&p.tasks_both("available").await?);
    ensure!(
        t.is_some_and(|t| t["claimable"] == true),
        "available after A is green"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}

// dependencies-before-priority-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac3_a_shared_parent_alone_never_makes_work_wait_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1"]).await?;
    // Controls: A1 and B1 are grey, and B1 is ready.
    ensure!(p.status("criterion:a1").await?["status"] == "grey");
    ensure!(p.status("criterion:b1").await?["status"] == "grey");
    let readiness = p.readiness("criterion:b1").await?;
    ensure!(readiness[0] == "ready", "{readiness}");
    let free =
        |b1: &Value| b1["work_ready"] == true && b1["process"]["readiness_blocker"].is_null();
    ensure!(p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    let b1 = p.object_both("criterion:b1").await?;
    ensure!(free(&b1), "{b1}");
    // Control: a blocking link from B to A makes B1 wait until it is removed.
    p.link("spec:b", ordering_link("spec:a")).await?;
    ensure!(!p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    p.link("spec:b", json!([])).await?;
    ensure!(p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    let b1 = p.object_both("criterion:b1").await?;
    ensure!(free(&b1), "{b1}");
    ensure!(p.readiness("criterion:b1").await? == readiness);
    Ok(())
}

// dependencies-before-priority-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac4_a_red_target_criterion_keeps_dependents_waiting_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1", "criterion:a2"]).await?;
    for (ac, test) in [("criterion:a1", A), ("criterion:a2", B)] {
        p.write(
            "agent",
            json!({"op":"set_ac_tests","target":ac,"tests":[test],"reason":"Bind the test"}),
        )
        .await?;
    }
    p.link("spec:b", ordering_link("spec:a")).await?;
    p.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    // Controls: A1 and A2 are green, B1 is ready and grey, and B1 is listed.
    for ac in ["criterion:a1", "criterion:a2"] {
        ensure!(p.status(ac).await?["status"] == "green", "{ac}");
    }
    ensure!(p.status("criterion:b1").await?["status"] == "grey");
    let readiness = p.readiness("criterion:b1").await?;
    ensure!(readiness[0] == "ready", "{readiness}");
    ensure!(p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    // A record fails the test of A2.
    p.record(
        "c2c2c2c",
        outcomes(&[("opens", "pass"), ("refuses", "fail")]),
    )
    .await?;
    ensure!(p.status("criterion:a2").await?["status"] == "red");
    ensure!(!p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    let b1 = p.object_both("criterion:b1").await?;
    ensure!(waits_for(&b1, "spec:a"), "{b1}");
    // A later record passes every listed test of A.
    p.record(
        "c3c3c3c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(p.status("criterion:a2").await?["status"] == "green");
    ensure!(p.unstarted().await?.contains(&"criterion:b1".to_owned()));
    Ok(())
}

// dependencies-before-priority-ac6
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac6_a_dependency_cycle_through_the_tree_is_refused_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1"]).await?;
    p.link("spec:b", ordering_link("spec:a")).await?;
    let save = |key: &str, object: &Value, links: Value| {
        let (kind, id) = key.split_once(':').unwrap();
        json!({"op":"save_object","kind":kind,"id":id,"revision":object["current_revision"],"parent":object["document"]["parent"],
            "data":object["document"]["data"],"links":links,"reason":"Order the work"})
    };
    // Each refusal leaves the committed rows and the journal position unchanged.
    for (case, source, target) in [
        ("fixture 1: A1 waits for B1", "criterion:a1", "criterion:b1"),
        ("fixture 2: A waits for B", "spec:a", "spec:b"),
    ] {
        let object = p.read(source).await?;
        p.refuse_command_as(
            &w.owner,
            case,
            save(source, &object, ordering_link(target)),
            "dependency cycle",
        )
        .await?;
        ensure!(
            p.object_both(source).await?["document"]["links"] == json!([]),
            "{case}"
        );
    }
    // Controls: a related link, a blocking link to an unrelated requirement and the removal
    // of the link from B to A make no cycle.
    p.save(
        "spec:d",
        Some("cap:c"),
        json!({"title":"spec:d","body":"Deliver another rule","owner":"owner"}),
        json!([]),
    )
    .await?;
    p.link("spec:a", json!([{"target":"spec:b","relation":"related"}]))
        .await?;
    p.link("criterion:b1", ordering_link("spec:d")).await?;
    p.link("spec:b", json!([])).await?;
    ensure!(p.read("criterion:b1").await?["document"]["links"] == ordering_link("spec:d"));
    ensure!(p.read("spec:b").await?["document"]["links"] == json!([]));
    store::check_journal(&p.pool).await?;
    Ok(())
}

// dependencies-before-priority-ac7
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac7_an_ordering_link_greys_nothing_and_an_archived_target_frees_work() -> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:d", &["criterion:a1"]).await?;
    p.write(
        "agent",
        json!({"op":"set_ac_tests","target":"criterion:b1","tests":[A],"reason":"Bind the test"}),
    )
    .await?;
    // check-list-frozen-ac3: the list save clears readiness, so B1 is assessed after it.
    let ready = json!({"ready":true,"reason":"Known scope"});
    p.confirm("spec:b", &[("criterion:b1", ready)]).await?;
    p.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    ensure!(p.status("criterion:b1").await?["status"] == "green");
    ensure!(p.status("criterion:a1").await?["status"] == "grey");
    let readiness = p.readiness("criterion:b1").await?;
    ensure!(readiness[0] == "ready", "{readiness}");
    let before = p.saved().await?["position"].as_i64().unwrap();

    // P adds a blocking link from B to A without a revision.
    p.link("spec:b", ordering_link("spec:a")).await?;
    ensure!(p.status("criterion:b1").await?["status"] == "green");
    ensure!(p.readiness("criterion:b1").await? == readiness);
    let b = p.read("spec:b").await?;
    let stored = &b["document"]["links"][0];
    ensure!(
        stored["target"] == "spec:a"
            && stored["blocking"] == true
            && stored.get("revision").is_none(),
        "{b}"
    );
    let events = p.saved().await?["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["position"].as_i64() > Some(before))
        .cloned()
        .collect::<Vec<_>>();
    ensure!(
        events.len() == 1
            && events[0]["actor"] == "owner"
            && events[0]["command"]["op"] == "save_object"
            && events[0]["command"]["kind"] == "spec"
            && events[0]["command"]["id"] == "b"
            && events[0]["command"]["reason"] == "Order the work"
            && events[0]["command"]["links"][0].get("revision").is_none(),
        "{events:?}"
    );
    // The timeline reader is absent from this checkout, so its HTTP and MCP check is pending.

    let b1 = p.object_both("criterion:b1").await?;
    ensure!(waits_for(&b1, "spec:a"), "{b1}");
    // P archives A1, so A has no live criterion and still holds B1. Then P archives A.
    for (target, held) in [("criterion:a1", true), ("spec:a", false)] {
        let revision = p.revision(target).await?;
        p.owner_command(json!({"op":"archive_object","target":target,"revision":revision,"reason":"Retire the scope"}))
            .await?;
        let b1 = p.object_both("criterion:b1").await?;
        let free = b1["work_ready"] == true
            && b1["process"]["readiness_blocker"].is_null()
            && b1.get("waits_for").is_none();
        ensure!(
            if held { waits_for(&b1, "spec:a") } else { free },
            "after archiving {target}: {b1}"
        );
    }

    // A text change of B is a change of B. A link to the archived A cannot be saved again.
    let object = p.read("spec:b").await?;
    p.owner_command(json!({"op":"save_object","kind":"spec","id":"b","revision":object["current_revision"],"parent":"cap:c",
        "data":{"title":"spec:b","body":"Deliver the sharper rule","owner":"owner"},"links":[],"reason":"Sharpen the rule"}))
        .await?;
    ensure!(p.status("criterion:b1").await?["status"] == "grey");
    store::check_journal(&p.pool).await?;
    Ok(())
}

// dependencies-before-priority-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn ac5_the_dependency_route_refuses_foreign_origin_non_members_and_cookie_commands()
-> Result<()> {
    let w = world().await?;
    let p = ordering(&w, "cap:c", &["criterion:a1"]).await?;
    let root = w.base.split("/api/").next().context("root")?.to_owned();
    let route = format!("{root}/api/workspaces/{}", p.ws);
    let session = |actor: &'static str| {
        let pool = p.pool.clone();
        async move {
            let session = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            sessions::create(&pool, &session, actor, None).await?;
            anyhow::Ok(session)
        }
    };
    let send = |session: String, origin: String, path: &'static str, body: Value| {
        let request = w
            .http
            .post(format!("{route}{path}"))
            .header("cookie", format!("joppa_session={session}"))
            .header("origin", origin)
            .json(&body);
        async move {
            let reply = request.send().await?;
            anyhow::Ok((
                reply.status().as_u16(),
                reply.json::<Value>().await.unwrap_or(Value::Null),
            ))
        }
    };
    let add = || json!({"request_id":uuid::Uuid::new_v4().to_string(),"source":"spec:b","target":"spec:a","action":"add"});
    let member = session("owner").await?;
    let outsider = session("outsider").await?;
    let b = p.read("spec:b").await?;
    let before = p.saved().await?;
    let refusals = [
        (
            "foreign Origin",
            send(
                member.clone(),
                "http://evil.example".into(),
                "/dependencies",
                add(),
            )
            .await?,
            403,
        ),
        (
            "non-member cookie session",
            send(outsider, root.clone(), "/dependencies", add()).await?,
            403,
        ),
        (
            "member cookie dependencies",
            send(member.clone(), root.clone(), "/dependencies", add()).await?,
            403,
        ),
        (
            "member cookie priority",
            send(
                member.clone(),
                root.clone(),
                "/priority",
                json!({"target":"spec:b","priority":"p0"}),
            )
            .await?,
            403,
        ),
        (
            "cookie commands",
            send(member.clone(), root.clone(), "/commands",
                json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"save_object","kind":"spec","id":"b",
                    "revision":b["current_revision"],"parent":"cap:c","data":b["document"]["data"],
                    "links":ordering_link("spec:a"),"reason":"Cookie write"}})).await?,
            403,
        ),
    ];
    for (case, (code, reply), expected) in refusals {
        ensure!(code == expected, "{case}: {code} {reply}");
        ensure!(p.saved().await? == before, "{case} changed saved state");
    }
    ensure!(p.read("spec:b").await?["document"]["links"] == json!([]));
    store::check_journal(&p.pool).await?;
    Ok(())
}

/// The addresses on one Task view page.
fn task_addresses(tasks: &[Value]) -> Vec<&str> {
    tasks.iter().filter_map(|t| t["address"].as_str()).collect()
}

/// T is done, needs no review and has no review label.
fn done_without_review(t: &Value) -> bool {
    t["process"]["status"] == "done"
        && t["process"]["review_required"] == false
        && t["signal"]["label"]
            .as_str()
            .is_some_and(|l| !l.contains("needs owner review"))
}

// completion-record-ac4
// Bot completes T under A. Then owner S saves a new text of R, then of A.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_done_task_stays_done_without_review_after_new_texts_of_r_and_a() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.ready().await?;
    p.save("work:t", Some("criterion:a"), json!({"title":"T","owner":"owner","contract":"Implement this criterion","purpose":"implementation"}), json!([])).await?;
    p.write(
        "agent",
        json!({"op":"claim_task","id":"work:t","revision":p.revision("work:t").await?}),
    )
    .await?;
    p.write("agent", json!({"op":"complete_task","commit":"c0ffee0","id":"work:t","revision":p.revision("work:t").await?,"evidence":"commit abc123 on master"})).await?;
    let t = p.object_both("work:t").await?;
    ensure!(done_without_review(&t), "T after completion: {t}");
    let spec = p.revision("spec:one").await?;
    p.owner_command(json!({"op":"save_object","kind":"spec","id":"one","revision":spec,"parent":null,
        "data":{"title":"One rule","body":"Implement the changed rule","owner":"owner"},"links":[],"reason":"Change R"}))
        .await?;
    let criterion = p.revision("criterion:a").await?;
    p.owner_command(json!({"op":"save_object","kind":"criterion","id":"a","revision":criterion,"parent":"spec:one",
        "data":{"title":"criterion:a","owner":"owner","description":"Changed wording"},"links":[],"reason":"Change A"}))
        .await?;
    // Controls: R and A are at new revisions.
    ensure!(p.revision("spec:one").await? > spec);
    ensure!(p.revision("criterion:a").await? == criterion + 1);
    ensure!(p.read("criterion:a").await?["document"]["data"]["description"] == "Changed wording");
    let t = p.object_both("work:t").await?;
    ensure!(done_without_review(&t), "T after the new texts: {t}");
    let all = p.tasks_both("all").await?;
    let entry = all
        .iter()
        .find(|t| t["address"] == "work:t")
        .ok_or_else(|| anyhow::anyhow!("T is missing in view=all: {all:?}"))?;
    ensure!(
        entry["status"] == "done"
            && entry["review_required"] == false
            && entry["signal"]["label"]
                .as_str()
                .is_some_and(|l| !l.contains("needs owner review")),
        "T in view=all: {entry}"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}

// pins-ignore-process-events-ac3
// Task B blocks on criterion C, and C lists test A. Records make C green, then red.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_green_criterion_frees_a_blocked_task_and_a_red_one_makes_it_wait() -> Result<()> {
    let w = world().await?;
    let p = planning(&w).await?;
    p.ready().await?;
    let pin = p.prerequisite("Deliver the prerequisite").await?;
    p.write(
        "agent",
        json!({"op":"set_ac_tests","target":"criterion:c","tests":[A],"reason":"Bind the test"}),
    )
    .await?;
    // The new test list resets the readiness of C, so the owner confirms C ready again.
    p.confirm(
        "spec:two",
        &[("criterion:c", json!({"ready":true,"reason":"Known scope"}))],
    )
    .await?;
    // The list save gives C a revision that changes only its tests, so the pin stays current.
    ensure!(p.revision("criterion:c").await? == pin + 1);
    p.save("work:b", Some("criterion:a"), json!({"title":"B","owner":"owner","contract":"Use the prerequisite","purpose":"implementation"}),
        json!([{"target":"criterion:c","relation":"depends_on","revision":pin,"blocking":true}])).await?;
    // Control: B waits while C is grey.
    ensure!(p.status("criterion:c").await?["status"] == "grey");
    ensure!(!task_addresses(&p.tasks_both("available").await?).contains(&"work:b"));
    ensure!(task_addresses(&p.tasks_both("waiting").await?).contains(&"work:b"));
    p.record(
        "c1c1c1c",
        outcomes(&[("opens", "pass"), ("refuses", "pass")]),
    )
    .await?;
    let c = p.status("criterion:c").await?;
    ensure!(c["status"] == "green", "C after the passing run: {c}");
    let available = p.tasks_both("available").await?;
    ensure!(
        task_addresses(&available).contains(&"work:b"),
        "B must be available: {available:?}"
    );
    let b = p.object_both("work:b").await?;
    ensure!(
        b["review_required"] == false && b["process"]["assignee"].is_null(),
        "B while C is green: {b}"
    );
    // Fixture 2: a later record fails test A.
    p.record(
        "c2c2c2c",
        outcomes(&[("opens", "fail"), ("refuses", "pass")]),
    )
    .await?;
    let c = p.status("criterion:c").await?;
    ensure!(c["status"] == "red", "C after the failing run: {c}");
    let available = p.tasks_both("available").await?;
    ensure!(
        !task_addresses(&available).contains(&"work:b"),
        "B must leave the available list: {available:?}"
    );
    let waiting = p.tasks_both("waiting").await?;
    ensure!(
        task_addresses(&waiting).contains(&"work:b"),
        "B must wait: {waiting:?}"
    );
    store::check_journal(&p.pool).await?;
    Ok(())
}

// pins-ignore-process-events-ac2
// Task B depends on open Task A. Planner P saves a new contract of A over HTTP, then over MCP.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_contract_of_a_prerequisite_task_makes_its_dependent_wait_for_review() -> Result<()> {
    let w = world().await?;
    let contract = |title: &str, contract: &str| json!({"title":title,"owner":"owner","contract":contract,"purpose":"implementation"});
    for mcp in [false, true] {
        let p = planning(&w).await?;
        p.ready().await?;
        p.save(
            "work:a",
            Some("criterion:a"),
            contract("A", "Deliver the prerequisite"),
            json!([]),
        )
        .await?;
        let pin = p.revision("work:a").await?;
        p.save(
            "work:b",
            Some("criterion:a"),
            contract("B", "Use the prerequisite"),
            json!([{"target":"work:a","relation":"depends_on","revision":pin,"blocking":true}]),
        )
        .await?;
        // Control: B pins the current revision of A.
        let b = p.object_both("work:b").await?;
        ensure!(b["review_required"] == false, "B before the change: {b}");
        p.claim("agent", "criterion:a").await?;
        let save = json!({"op":"save_object","kind":"work","id":"a","revision":pin,"parent":"criterion:a",
            "data":contract("A","Deliver the changed prerequisite"),"links":[],"reason":"Change the contract"});
        p.send(mcp, &save).await?.map_err(anyhow::Error::msg)?;
        let a = p.object_both("work:a").await?;
        ensure!(
            a["current_revision"] == pin + 1 && a["process"]["status"] == "open",
            "A after the change: {a}"
        );
        let b = p.object_both("work:b").await?;
        ensure!(
            b["review_required"] == true && b["document"]["links"][0]["revision"] == pin,
            "B after the change: {b}"
        );
        let waiting = p.tasks_both("waiting").await?;
        let entry = waiting
            .iter()
            .find(|t| t["address"] == "work:b")
            .ok_or_else(|| anyhow::anyhow!("B is missing in view=waiting: {waiting:?}"))?;
        ensure!(
            entry["claimable"] == false
                && entry["waiting_reason"]
                    == "Task has an unmet dependency or a changed dependency revision: work:a",
            "B in view=waiting: {entry}"
        );
        let available = p.tasks_both("available").await?;
        ensure!(
            !task_addresses(&available).contains(&"work:b"),
            "B must not be available: {available:?}"
        );
        store::check_journal(&p.pool).await?;
    }
    Ok(())
}
