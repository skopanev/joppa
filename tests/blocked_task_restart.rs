//! Blocked Tasks keep their block state and queue availability, and claimed and done
//! Tasks keep their holder, history, comments and test results, when a real server
//! restarts and rebuilds the workspace projections from the journal.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
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
async fn start(url: &str, tokens: &std::path::Path, port: u16) -> Result<Server> {
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("DEEPINFRA_API_KEY", "test-only-key")
            .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
            .env("JOPPA_DATABASE_URL", url)
            .env("JOPPA_TOKENS_FILE", tokens)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    for _ in 0..100 {
        if reqwest::get(format!("http://127.0.0.1:{port}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(server);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("test server did not start")
}

/// Derived tables that server startup must rebuild from the journal.
const TABLES: [&str; 8] = [
    "objects",
    "object_revisions",
    "object_links",
    "ac_owners",
    "state_entries",
    "state_revisions",
    "task_queue",
    "search_documents",
];
const BLOCKED: &str = "Task is blocked for planning";
const PLANNER: &str = "planner";
const L: &str = "worker-l";
const Z: &str = "worker-z";
const OWNER: &str = "owner";
/// Each fixture Task under its own ready criterion.
const FIXTURES: [(&str, &str, &str); 4] = [
    ("F1", "work:t1", "criterion:f1"),
    ("F2", "work:t2", "criterion:f2"),
    ("F3", "work:d", "criterion:f3"),
    ("F4", "work:t4", "criterion:f4"),
];

struct Fixture {
    pool: deadpool_postgres::Pool,
    ws: String,
}
impl Fixture {
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
        .map_err(|e| anyhow::anyhow!("{actor}: {command}: {e:#}"))
    }
    async fn read(&self, address: &str) -> Result<Value> {
        store::read_objects(
            &self.pool,
            &self.ws,
            OWNER,
            &store::ObjectScope {
                address: Some(address),
                ..Default::default()
            },
        )
        .await
    }
    /// `actor` sends `command` for `id` at its current revision.
    async fn task(&self, actor: &str, id: &str, mut command: Value) -> Result<Value> {
        command["id"] = json!(id);
        command["revision"] = self.read(id).await?["current_revision"].clone();
        self.write(actor, command).await
    }
    async fn block(&self, actor: &str, id: &str) -> Result<Value> {
        let reply = self.task(actor, id, json!({"op":"block_task","reason":"Verification failed","evidence":"test x failed on abc123"})).await?;
        Ok(
            json!({"actor":actor,"reason":"Verification failed","evidence":"test x failed on abc123","position":reply["position"]}),
        )
    }
    /// Remove the workspace projections and its board cache; keep the journal.
    async fn clear_projections(&self) -> Result<()> {
        {
            let conn = self.pool.get().await?;
            for table in TABLES {
                let statement = format!("DELETE FROM joppa.{table} WHERE workspace=$1");
                conn.execute(&statement, &[&self.ws]).await?;
            }
            conn.execute(
                "DELETE FROM joppa.board_cache WHERE workspace=$1",
                &[&self.ws],
            )
            .await?;
        }
        let cache = "SELECT count(*) FROM joppa.board_cache WHERE workspace=$1";
        let queue = "SELECT count(*) FROM joppa.task_queue WHERE workspace=$1";
        let (cached, queued) = (self.count(cache).await?, self.count(queue).await?);
        ensure!(
            cached == 0 && queued == 0,
            "projections remain before startup: board_cache={cached} task_queue={queued}"
        );
        Ok(())
    }
    async fn count(&self, statement: &str) -> Result<i64> {
        Ok(self
            .pool
            .get()
            .await?
            .query_one(statement, &[&self.ws])
            .await?
            .get(0))
    }
}

/// The normalized reads of one transport, keyed by fixture name. Only the compared
/// fields are kept, so no read time can differ between reads.
type Snapshot = BTreeMap<&'static str, Value>;

/// One read of the token member: the HTTP `path`, or the MCP `tool` with `arguments`.
async fn fetch(
    http: &reqwest::Client,
    root: &str,
    ws: &str,
    token: &str,
    mcp: bool,
    (path, tool, mut arguments): (String, &str, Value),
) -> Result<Value> {
    if !mcp {
        let response = http
            .get(format!("{root}/api/workspaces/{ws}/{path}"))
            .bearer_auth(token)
            .send()
            .await?;
        let code = response.status();
        let body: Value = response.json().await?;
        ensure!(code.is_success(), "HTTP {path}: {code} {body}");
        return Ok(body);
    }
    arguments["workspace"] = json!(ws);
    let response: Value = http.post(format!("{root}/mcp")).bearer_auth(token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}}))
        .send().await?.json().await?;
    ensure!(
        response["result"]["isError"] != true,
        "MCP {tool}: {response}"
    );
    Ok(serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("null"),
    )?)
}

async fn snapshot(
    http: &reqwest::Client,
    root: &str,
    ws: &str,
    token: &str,
    mcp: bool,
) -> Result<Snapshot> {
    let get = |path: String, tool: &'static str, arguments: Value| {
        fetch(http, root, ws, token, mcp, (path, tool, arguments))
    };
    let all = get(
        "tasks?view=all&limit=100".into(),
        "joppa_tasks",
        json!({"view":"all","limit":100}),
    )
    .await?;
    let available = get(
        "tasks?view=available&limit=100".into(),
        "joppa_tasks",
        json!({"view":"available","limit":100}),
    )
    .await?;
    let find = |page: &Value, address: &str| {
        page["tasks"]
            .as_array()
            .and_then(|tasks| tasks.iter().find(|t| t["address"] == address))
            .cloned()
    };
    let mut out = Snapshot::new();
    for (name, address, _) in FIXTURES {
        let object = get(
            format!("objects?address={address}"),
            "joppa_objects",
            json!({"address":address}),
        )
        .await?;
        let queued = find(&all, address)
            .ok_or_else(|| anyhow::anyhow!("{name} {address} is missing in view=all: {all}"))?;
        out.insert(
            name,
            json!({
                "status": object["process"]["status"],
                "blocked": object["process"]["blocked"],
                "assignee": object["process"]["assignee"],
                "claimable": queued["claimable"],
                "waiting_reason": queued["waiting_reason"],
                "available": find(&available, address).is_some(),
            }),
        );
    }
    Ok(out)
}

/// The fixture values that each transport must report at each point.
fn expect(
    point: &str,
    transport: &str,
    s: &Snapshot,
    blocks: &BTreeMap<&str, Value>,
) -> Result<()> {
    let at = |name: &str| format!("{point} {transport} {name}: {}", s[name]);
    let f1 = &s["F1"];
    ensure!(
        f1["blocked"] == blocks["F1"]
            && f1["blocked"]["actor"] == L
            && f1["assignee"].is_null()
            && f1["claimable"] == false
            && f1["waiting_reason"] == BLOCKED
            && f1["available"] == false,
        "{}",
        at("F1")
    );
    let f2 = &s["F2"];
    ensure!(
        f2["blocked"].is_null() && f2["waiting_reason"] != BLOCKED,
        "{}",
        at("F2")
    );
    let f3 = &s["F3"];
    ensure!(
        f3["blocked"] == blocks["F3"]
            && f3["blocked"]["actor"] == OWNER
            && f3["assignee"].is_null()
            && f3["claimable"] == false
            && f3["available"] == false,
        "{}",
        at("F3")
    );
    let f4 = &s["F4"];
    ensure!(
        f4["status"] == "retired"
            && f4["blocked"] == blocks["F4"]
            && f4["claimable"] == false
            && f4["available"] == false,
        "{}",
        at("F4")
    );
    Ok(())
}

// failed-task-blocks-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn blocked_tasks_keep_their_state_across_a_restart_that_rebuilds_the_journal() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let f = Fixture {
        pool,
        ws: format!("blocked-restart-{}", uuid::Uuid::new_v4().simple()),
    };

    // A workspace with custom criterion and Task type names and one ready criterion per fixture.
    store::execute_input(
        &f.pool,
        &f.ws,
        OWNER,
        "workspace",
        &json!({"op":"workspace","title":"Blocked Tasks across a restart"}),
    )
    .await?;
    for actor in [PLANNER, L, Z] {
        f.write(
            OWNER,
            json!({"op":"enroll_service","id":actor,"role":"agent"}),
        )
        .await?;
    }
    let schema = store::tree_schema(&f.pool, &f.ws, OWNER, None).await?;
    f.write(OWNER, json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    f.write(PLANNER, json!({"op":"save_object","kind":"spec","id":"one","revision":0,"parent":null,"data":{"title":"One rule","body":"Implement the agreed rule","owner":OWNER},"reason":"Set the rule"})).await?;
    for (_, _, criterion) in FIXTURES {
        let id = criterion.split_once(':').unwrap().1;
        f.write(PLANNER, json!({"op":"save_object","kind":"criterion","id":id,"revision":0,"parent":"spec:one","data":{"title":id,"owner":OWNER},"reason":"Set the criterion"})).await?;
    }
    let revision = f.read("spec:one").await?["process"]["revision"].clone();
    f.write(
        OWNER,
        json!({"op":"confirm","req":"spec:one","revision":revision}),
    )
    .await?;
    for (_, task, criterion) in FIXTURES {
        f.write(PLANNER, json!({"op":"assess","req":"spec:one","revision":revision,"ac":criterion,"assessment":{"ready":true,"reason":"Known scope"}})).await?;
        let id = task.split_once(':').unwrap().1;
        f.write(PLANNER, json!({"op":"save_object","kind":"work","id":id,"revision":0,"parent":criterion,"data":{"title":id,"owner":OWNER,"contract":"Implement the agreed mechanism","purpose":"implementation"},"reason":"Set the work contract"})).await?;
    }

    let mut blocks = BTreeMap::new();
    // F1: L claims and blocks T1.
    f.task(L, "work:t1", json!({"op":"claim_task"})).await?;
    blocks.insert("F1", f.block(L, "work:t1").await?);
    // F2: L blocks T2; P plans a new T2 contract and finishes planning.
    f.task(L, "work:t2", json!({"op":"claim_task"})).await?;
    f.block(L, "work:t2").await?;
    let ac = f.read("criterion:f2").await?;
    f.write(
        PLANNER,
        json!({"op":"claim_planning","ac":"criterion:f2","revision":ac["current_revision"]}),
    )
    .await?;
    {
        let current = f.read("work:t2").await?;
        let mut data = current["document"]["data"].clone();
        data["title"] = json!("t2");
        data["contract"] = json!("Implement the agreed mechanism with a new fix");
        f.write(PLANNER, json!({"op":"save_object","kind":"work","id":"t2","revision":current["current_revision"],"parent":current["document"]["parent"],"data":data,"links":current["document"]["links"],"reason":"Amend the contract"})).await?;
    }
    f.write(
        PLANNER,
        json!({"op":"finish_planning","ac":"criterion:f2","reason":"Plan complete"}),
    )
    .await?;
    let t2 = f.read("work:t2").await?;
    ensure!(t2["process"]["blocked"].is_null(), "F2: {t2}");
    blocks.insert("F2", Value::Null);
    // F3: Z claims D and the workspace owner S blocks it.
    f.task(Z, "work:d", json!({"op":"claim_task"})).await?;
    blocks.insert("F3", f.block(OWNER, "work:d").await?);
    let d = f.read("work:d").await?;
    ensure!(
        d["process"]["blocked"]["actor"] == OWNER && d["process"]["assignee"].is_null(),
        "F3: {d}"
    );
    // F4: L blocks T4 and S retires it.
    f.task(L, "work:t4", json!({"op":"claim_task"})).await?;
    let block = f.block(L, "work:t4").await?;
    f.task(
        OWNER,
        "work:t4",
        json!({"op":"retire_task","reason":"The planned work replaces this Task"}),
    )
    .await?;
    let t4 = f.read("work:t4").await?;
    ensure!(
        t4["process"]["status"] == "retired" && t4["process"]["blocked"] == block,
        "F4: {t4}"
    );
    blocks.insert("F4", block);

    // Worker Z reads the workspace through one bearer token on HTTP and MCP.
    let dir = std::env::temp_dir().join(format!(
        "joppa-blocked-restart-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir)?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, json!({ token.clone(): Z }).to_string())?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let read = || async {
        let mut out = BTreeMap::new();
        for (transport, mcp) in [("HTTP", false), ("MCP", true)] {
            out.insert(transport, snapshot(&http, &root, &f.ws, &token, mcp).await?);
        }
        Ok::<_, anyhow::Error>(out)
    };
    let check = |point: &str, out: &BTreeMap<&str, Snapshot>| {
        for (transport, s) in out {
            expect(point, transport, s, &blocks)?;
        }
        ensure!(
            out["HTTP"] == out["MCP"],
            "{point}: HTTP {:#?} differs from MCP {:#?}",
            out["HTTP"],
            out["MCP"]
        );
        Ok::<_, anyhow::Error>(())
    };

    let server = start(&url, &tokens, port).await?;
    let before = read().await?;
    check("before the restart", &before)?;
    store::check_journal(&f.pool).await?;
    drop(server);

    f.clear_projections().await?;

    let server = start(&url, &tokens, port).await?;
    let rebuilt = f
        .count("SELECT count(*) FROM joppa.board_cache b WHERE b.workspace=$1 AND b.position=(SELECT MAX(e.position) FROM joppa.events e WHERE e.workspace=b.workspace)")
        .await?;
    ensure!(
        rebuilt == 1,
        "startup did not rebuild board_cache at the last event position"
    );
    let rows = f
        .pool
        .get()
        .await?
        .query(
            "SELECT DISTINCT address FROM joppa.task_queue WHERE workspace=$1",
            &[&f.ws],
        )
        .await?;
    let addresses: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
    for (name, address, _) in FIXTURES {
        ensure!(
            addresses.iter().any(|a| a == address),
            "startup did not rebuild the task_queue row of {name} {address}: {addresses:?}"
        );
    }
    let after = read().await?;
    drop(server);
    // Report a restart difference before the fixture values are checked again.
    for (transport, snapshot) in &before {
        for (name, fields) in snapshot {
            for (field, old) in fields.as_object().unwrap() {
                let new = &after[transport][name][field];
                ensure!(
                    old == new,
                    "{name} {transport} {field}: before the restart {old}, after {new}"
                );
            }
        }
    }
    check("after the restart", &after)?;
    store::check_journal(&f.pool).await?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

const READER: &str = "reader";
const AWAITING: &str = "Task is awaiting planning publication";
/// Planning fixtures with custom type names: each criterion and its new Task.
const PLANS: [(&str, &str, Option<&str>); 4] = [
    ("held", "rule:held", Some("job:held")),
    ("finished", "rule:finished", Some("job:finished")),
    ("released", "rule:released", Some("job:released")),
    ("claimed", "rule:claimed", None),
];

/// The planning claim, unstarted work, Task state and queue values of each plan.
async fn planning_snapshot(
    http: &reqwest::Client,
    root: &str,
    ws: &str,
    token: &str,
    mcp: bool,
) -> Result<Snapshot> {
    let get = |path: String, tool: &'static str, arguments: Value| {
        fetch(http, root, ws, token, mcp, (path, tool, arguments))
    };
    let mut pages = BTreeMap::new();
    for view in ["all", "available"] {
        let page = get(
            format!("tasks?view={view}&limit=100"),
            "joppa_tasks",
            json!({"view":view,"limit":100}),
        )
        .await?;
        pages.insert(view, page["tasks"].clone());
    }
    let unstarted = get(
        "objects?work_filter=unstarted&limit=100".into(),
        "joppa_objects",
        json!({"work_filter":"unstarted","limit":100}),
    )
    .await?;
    pages.insert("unstarted", unstarted["items"].clone());
    let find = |page: &str, address: &str| {
        pages[page]
            .as_array()
            .and_then(|items| items.iter().find(|i| i["address"] == address))
            .cloned()
    };
    let mut out = Snapshot::new();
    for (name, criterion, task) in PLANS {
        let object = |address: &str| {
            get(
                format!("objects?address={address}"),
                "joppa_objects",
                json!({"address":address}),
            )
        };
        let c = object(criterion).await?;
        let mut fields = json!({
            "planning_claim": c["process"]["planning_claim"],
            "unstarted": find("unstarted", criterion).is_some(),
        });
        if let Some(task) = task {
            let t = object(task).await?;
            let queued = find("all", task)
                .ok_or_else(|| anyhow::anyhow!("{name} {task} is missing in view=all"))?;
            fields["status"] = t["process"]["status"].clone();
            fields["assignee"] = t["process"]["assignee"].clone();
            fields["claimable"] = queued["claimable"].clone();
            fields["waiting_reason"] = queued["waiting_reason"].clone();
            fields["available"] = json!(find("available", task).is_some());
        }
        out.insert(name, fields);
    }
    Ok(out)
}

/// The plan values that each transport must report at each point.
fn expect_plans(
    point: &str,
    transport: &str,
    s: &Snapshot,
    claims: &BTreeMap<&str, Value>,
) -> Result<()> {
    let expected = BTreeMap::from([
        (
            "held",
            json!({"planning_claim":claims["held"],"unstarted":false,"status":"open","assignee":null,
                "claimable":false,"waiting_reason":AWAITING,"available":false}),
        ),
        (
            "finished",
            json!({"planning_claim":null,"unstarted":false,"status":"open","assignee":null,
                "claimable":true,"waiting_reason":null,"available":true}),
        ),
        (
            "released",
            json!({"planning_claim":null,"unstarted":true,"status":"retired","assignee":null,
                "claimable":false,"waiting_reason":"task is retired","available":false}),
        ),
        (
            "claimed",
            json!({"planning_claim":claims["claimed"],"unstarted":false}),
        ),
    ]);
    for (name, fields) in &expected {
        ensure!(
            &s[name] == fields,
            "{point} {transport} {name}: expected {fields}, got {}",
            s[name]
        );
    }
    Ok(())
}

// planning-claim-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn held_finished_and_released_plans_keep_their_state_across_a_journal_rebuild() -> Result<()>
{
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let f = Fixture {
        pool,
        ws: format!("planning-restart-{}", uuid::Uuid::new_v4().simple()),
    };

    // Custom type names with criterion and Task roles; every rule is ready.
    store::execute_input(
        &f.pool,
        &f.ws,
        OWNER,
        "workspace",
        &json!({"op":"workspace","title":"Plans across a restart"}),
    )
    .await?;
    for actor in [PLANNER, READER] {
        f.write(
            OWNER,
            json!({"op":"enroll_service","id":actor,"role":"agent"}),
        )
        .await?;
    }
    let schema = store::tree_schema(&f.pool, &f.ws, OWNER, None).await?;
    f.write(OWNER, json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "story":{"title":"Story","role":"requirement","status":"children","root":true,"parents":[]},
        "rule":{"title":"Rule","role":"ac","status":"tests","parents":["story"]},
        "job":{"title":"Job","role":"task","status":"none","parents":["rule"],"parent_gate":"ready"}
    }})).await?;
    f.write(OWNER, json!({"op":"save_object","kind":"story","id":"one","revision":0,"parent":null,"data":{"title":"One rule","body":"Implement the agreed rule","owner":OWNER},"reason":"Set the rule"})).await?;
    for (_, criterion, _) in PLANS {
        let id = criterion.split_once(':').unwrap().1;
        f.write(OWNER, json!({"op":"save_object","kind":"rule","id":id,"revision":0,"parent":"story:one","data":{"title":id,"owner":OWNER},"reason":"Set the criterion"})).await?;
    }
    let revision = f.read("story:one").await?["process"]["revision"].clone();
    f.write(
        OWNER,
        json!({"op":"confirm","req":"story:one","revision":revision}),
    )
    .await?;
    let mut claims = BTreeMap::new();
    for (name, criterion, task) in PLANS {
        f.write(OWNER, json!({"op":"assess","req":"story:one","revision":revision,"ac":criterion,"assessment":{"ready":true,"reason":"Known scope"}})).await?;
        let rule = f.read(criterion).await?;
        let reply = f
            .write(
                PLANNER,
                json!({"op":"claim_planning","ac":criterion,"revision":rule["current_revision"]}),
            )
            .await?;
        claims.insert(name, json!({"actor":PLANNER,"position":reply["position"]}));
        if let Some(task) = task {
            let id = task.split_once(':').unwrap().1;
            f.write(PLANNER, json!({"op":"save_object","kind":"job","id":id,"revision":0,"parent":criterion,"data":{"title":id,"owner":OWNER,"contract":"Implement the agreed mechanism","purpose":"implementation"},"reason":"Plan the work"})).await?;
        }
    }
    f.write(
        PLANNER,
        json!({"op":"finish_planning","ac":"rule:finished","reason":"Plan complete"}),
    )
    .await?;
    f.write(
        PLANNER,
        json!({"op":"release_planning","ac":"rule:released","reason":"Plan withdrawn"}),
    )
    .await?;

    // Reader R reads the workspace through one bearer token on HTTP and MCP.
    let dir = std::env::temp_dir().join(format!(
        "joppa-planning-restart-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir)?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, json!({ token.clone(): READER }).to_string())?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let read = || async {
        let mut out = BTreeMap::new();
        for (transport, mcp) in [("HTTP", false), ("MCP", true)] {
            out.insert(
                transport,
                planning_snapshot(&http, &root, &f.ws, &token, mcp).await?,
            );
        }
        Ok::<_, anyhow::Error>(out)
    };
    let check = |point: &str, out: &BTreeMap<&str, Snapshot>| {
        for (transport, s) in out {
            expect_plans(point, transport, s, &claims)?;
        }
        ensure!(
            out["HTTP"] == out["MCP"],
            "{point}: HTTP {:#?} differs from MCP {:#?}",
            out["HTTP"],
            out["MCP"]
        );
        Ok::<_, anyhow::Error>(())
    };

    let server = start(&url, &tokens, port).await?;
    let before = read().await?;
    check("before the restart", &before)?;
    store::check_journal(&f.pool).await?;
    drop(server);
    f.clear_projections().await?;
    let server = start(&url, &tokens, port).await?;
    let after = read().await?;
    drop(server);
    ensure!(
        after == before,
        "the restart changed the plans: before {before:#?}, after {after:#?}"
    );
    check("after the restart", &after)?;
    store::check_journal(&f.pool).await?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

const HOLDER: &str = "worker-p";
/// The two tests that criterion A binds, in the recorded order.
const CHECKS: [&str; 2] = [
    "tests/auth/login.test.ts > login > opens",
    "tests/auth/login.test.ts > login > refuses",
];

/// Every read that the restart must keep on one transport: the Task fields, the
/// revision history of each object and, on HTTP, the test reply of criterion A.
async fn journal_snapshot(
    http: &reqwest::Client,
    root: &str,
    ws: &str,
    token: &str,
    mcp: bool,
    addresses: &[String],
) -> Result<BTreeMap<String, Value>> {
    let get = |path: String, tool: &'static str, arguments: Value| {
        fetch(http, root, ws, token, mcp, (path, tool, arguments))
    };
    let mut out = BTreeMap::new();
    for task in ["work:t1", "work:t2"] {
        let t = get(
            format!("objects?address={task}"),
            "joppa_objects",
            json!({"address":task}),
        )
        .await?;
        // HTTP keeps comments as a map by ID; MCP gives a page of items. Both give the
        // same ID, text, actor and reply.
        let entries: Vec<(String, Value)> = match t["comments"]["items"].as_array() {
            Some(items) => items
                .iter()
                .map(|c| (c["id"].as_str().unwrap_or_default().to_owned(), c.clone()))
                .collect(),
            None => t["annotations"]["comments"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(id, c)| (id.clone(), c.clone()))
                .collect(),
        };
        let comments: serde_json::Map<String, Value> = entries
            .into_iter()
            .map(|(id, c)| {
                let reply_to = c.get("reply_to").cloned().unwrap_or(Value::Null);
                (
                    id,
                    json!({"text": c["text"], "actor": c["actor"], "reply_to": reply_to}),
                )
            })
            .collect();
        out.insert(
            format!("task {task}"),
            json!({
                "status": t["process"]["status"],
                "assignee": t["process"]["assignee"],
                "current_revision": t["current_revision"],
                "comments": comments,
            }),
        );
    }
    for address in addresses {
        let history = get(
            format!("objects?address={address}&history=true"),
            "joppa_objects",
            json!({"address":address,"history":true}),
        )
        .await?;
        let count = history["revisions"].as_array().map_or(0, Vec::len) as u64;
        ensure!(
            count > 0 && history["current_revision"] == count,
            "{address}: no revisions: {history}"
        );
        out.insert(format!("history {address}"), history["revisions"].clone());
        for n in 1..=count {
            let revision = get(
                format!("objects?address={address}&revision={n}"),
                "joppa_objects",
                json!({"address":address,"revision":n}),
            )
            .await?;
            ensure!(
                revision["document"]["revision"] == n,
                "{address} revision {n}: {revision}"
            );
            // The time of the last status event is display state that a rebuild takes from
            // the document; the AC compares documents, revisions, comments and test results.
            let mut revision = revision;
            if let Some(signal) = revision["signal"].as_object_mut() {
                signal.remove("at");
            }
            out.insert(format!("revision {address} {n}"), revision);
        }
    }
    if !mcp {
        let tests = get("tests?ac=criterion:a".into(), "", Value::Null).await?;
        out.insert("tests criterion:a".into(), tests);
    }
    Ok(out)
}

/// Add one line to `out` for each JSON path below `at` where `old` and `new` differ.
fn differences(at: &str, old: &Value, new: &Value, out: &mut Vec<String>) {
    match (old, new) {
        (Value::Object(o), Value::Object(n)) => {
            for key in o.keys().chain(n.keys().filter(|k| !o.contains_key(*k))) {
                let missing = Value::Null;
                let (a, b) = (
                    o.get(key).unwrap_or(&missing),
                    n.get(key).unwrap_or(&missing),
                );
                if o.contains_key(key) != n.contains_key(key) {
                    out.push(format!("{at}.{key}: before {a}, after {b}"));
                } else {
                    differences(&format!("{at}.{key}"), a, b, out);
                }
            }
        }
        (Value::Array(o), Value::Array(n)) if o.len() == n.len() => {
            for (i, (a, b)) in o.iter().zip(n).enumerate() {
                differences(&format!("{at}[{i}]"), a, b, out);
            }
        }
        _ if old != new => out.push(format!("{at}: before {old}, after {new}")),
        _ => {}
    }
}

// claims-persist-replay
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_holder_done_work_comments_and_test_results_survive_a_journal_rebuild() -> Result<()> {
    let url = std::env::var("JOPPA_TEST_DATABASE_URL")?;
    let pool = store::pool(&url)?;
    let database: String = pool
        .get()
        .await?
        .query_one("SELECT current_database()", &[])
        .await?
        .get(0);
    ensure!(
        database.starts_with("joppa_test"),
        "isolated test database required"
    );
    store::migrate(&pool).await?;
    let f = Fixture {
        pool,
        ws: format!("claims-restart-{}", uuid::Uuid::new_v4().simple()),
    };

    // One ready criterion for each Task. Criterion A binds two tests.
    store::execute_input(
        &f.pool,
        &f.ws,
        OWNER,
        "workspace",
        &json!({"op":"workspace","title":"Claims across a restart"}),
    )
    .await?;
    f.write(
        OWNER,
        json!({"op":"enroll_service","id":HOLDER,"role":"agent"}),
    )
    .await?;
    let schema = store::tree_schema(&f.pool, &f.ws, OWNER, None).await?;
    f.write(OWNER, json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","status":"children","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","status":"tests","parents":["spec"]},
        "work":{"title":"Work","role":"task","status":"none","parents":["criterion"],"parent_gate":"ready"}
    }})).await?;
    f.write(OWNER, json!({"op":"save_object","kind":"spec","id":"one","revision":0,"parent":null,"data":{"title":"One rule","body":"Implement the agreed rule","owner":OWNER},"reason":"Set the rule"})).await?;
    for id in ["a", "b"] {
        f.write(OWNER, json!({"op":"save_object","kind":"criterion","id":id,"revision":0,"parent":"spec:one","data":{"title":id,"owner":OWNER},"reason":"Set the criterion"})).await?;
    }
    f.write(HOLDER, json!({"op":"set_ac_tests","target":"criterion:a","tests":CHECKS,"reason":"Bind the tests"})).await?;
    let revision = f.read("spec:one").await?["process"]["revision"].clone();
    f.write(
        OWNER,
        json!({"op":"confirm","req":"spec:one","revision":revision}),
    )
    .await?;
    for (task, criterion) in [("t1", "criterion:a"), ("t2", "criterion:b")] {
        f.write(OWNER, json!({"op":"assess","req":"spec:one","revision":revision,"ac":criterion,"assessment":{"ready":true,"reason":"Known scope"}})).await?;
        f.write(OWNER, json!({"op":"save_object","kind":"work","id":task,"revision":0,"parent":criterion,"data":{"title":task,"owner":OWNER,"contract":"Implement the agreed mechanism","purpose":"implementation"},"reason":"Set the work contract"})).await?;
    }
    // P completes T2 and then holds T1. T1 has a comment and a reply.
    f.task(HOLDER, "work:t2", json!({"op":"claim_task"}))
        .await?;
    f.task(
        HOLDER,
        "work:t2",
        json!({"op":"complete_task","commit":"c0ffee0","evidence":"Implemented the agreed mechanism"}),
    )
    .await?;
    f.task(HOLDER, "work:t1", json!({"op":"claim_task"}))
        .await?;
    f.write(
        OWNER,
        json!({"op":"note","target":"work:t1","id":"c1","text":"Keep the old sign-in path"}),
    )
    .await?;
    f.write(HOLDER, json!({"op":"note","target":"work:t1","id":"c2","text":"The old path stays","reply_to":"c1"}))
        .await?;

    // The owner reads the workspace through one bearer token on HTTP and MCP.
    let dir = std::env::temp_dir().join(format!(
        "joppa-claims-restart-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir)?;
    let (token, holder) = (
        uuid::Uuid::new_v4().simple().to_string(),
        uuid::Uuid::new_v4().simple().to_string(),
    );
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({ token.clone(): OWNER, holder.clone(): HOLDER }).to_string(),
    )?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let root = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let server = start(&url, &tokens, port).await?;
    // P records the results of both tests of A: one passes and one fails.
    let results = json!([{"test":CHECKS[0],"status":"pass"},{"test":CHECKS[1],"status":"fail","message":"refused"}]);
    record_http::recorded(&http, &root, &f.ws, &holder, "c1c1c1c", results).await?;

    let addresses = || async {
        let rows = f
            .pool
            .get()
            .await?
            .query(
                "SELECT address FROM joppa.objects WHERE workspace=$1 ORDER BY address",
                &[&f.ws],
            )
            .await?;
        Ok::<Vec<String>, anyhow::Error>(rows.iter().map(|r| r.get(0)).collect())
    };
    let listed = addresses().await?;
    for address in [
        "spec:one",
        "criterion:a",
        "criterion:b",
        "work:t1",
        "work:t2",
    ] {
        ensure!(
            listed.iter().any(|a| a == address),
            "{address} is missing: {listed:?}"
        );
    }
    let read = || async {
        let mut out = BTreeMap::new();
        for (transport, mcp) in [("HTTP", false), ("MCP", true)] {
            out.insert(
                transport,
                journal_snapshot(&http, &root, &f.ws, &token, mcp, &listed).await?,
            );
        }
        Ok::<_, anyhow::Error>(out)
    };
    let check = |point: &str, out: &BTreeMap<&str, BTreeMap<String, Value>>| {
        for (transport, s) in out {
            let t1 = &s["task work:t1"];
            let comments = &t1["comments"];
            ensure!(
                t1["status"] == "in_progress"
                    && t1["assignee"] == HOLDER
                    && comments.as_object().map(|c| c.len()) == Some(2)
                    && comments["c1"]["text"] == "Keep the old sign-in path"
                    && comments["c1"]["actor"] == OWNER
                    && comments["c2"]["text"] == "The old path stays"
                    && comments["c2"]["actor"] == HOLDER
                    && comments["c2"]["reply_to"] == "c1",
                "{point} {transport} T1: {t1}"
            );
            let t2 = &s["task work:t2"];
            ensure!(
                t2["status"] == "done" && t2["assignee"] == HOLDER,
                "{point} {transport} T2: {t2}"
            );
        }
        for task in ["task work:t1", "task work:t2"] {
            ensure!(
                out["HTTP"][task] == out["MCP"][task],
                "{point} {task}: HTTP {} differs from MCP {}",
                out["HTTP"][task],
                out["MCP"][task]
            );
        }
        let tests = &out["HTTP"]["tests criterion:a"];
        let results: Vec<(&str, &str)> = tests["tests"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| Some((t["address"].as_str()?, t["status"].as_str()?)))
            .collect();
        ensure!(
            results == [(CHECKS[0], "pass"), (CHECKS[1], "fail")] && tests["status"] == "red",
            "{point} GET /tests?ac=criterion:a: {tests}"
        );
        Ok::<_, anyhow::Error>(())
    };

    let before = read().await?;
    check("before the restart", &before)?;
    store::check_journal(&f.pool).await?;
    drop(server);
    f.clear_projections().await?;
    let server = start(&url, &tokens, port).await?;
    ensure!(
        addresses().await? == listed,
        "startup did not rebuild every object"
    );
    let after = read().await?;
    drop(server);
    // Report every restart difference by its read and field before the fixture values are checked.
    let mut changed = vec![];
    for (transport, reads) in &before {
        ensure!(
            after[transport].keys().eq(reads.keys()),
            "{transport}: the restart changed the set of reads"
        );
        for (name, old) in reads {
            differences(
                &format!("{transport} {name}"),
                old,
                &after[transport][name],
                &mut changed,
            );
        }
    }
    ensure!(
        changed.is_empty(),
        "the restart changed {} values:\n{}",
        changed.len(),
        changed.join("\n")
    );
    check("after the restart", &after)?;
    store::check_journal(&f.pool).await?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
