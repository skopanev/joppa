//! Blocked Tasks keep their block state and queue availability when a real server
//! restarts and rebuilds the workspace projections from the journal.
use anyhow::{Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
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
async fn start(url: &str, tokens: &std::path::Path, port: u16) -> Result<Server> {
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
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
        f.write(OWNER, json!({"op":"member","id":actor,"role":"agent"}))
            .await?;
    }
    let schema = store::tree_schema(&f.pool, &f.ws, OWNER, None).await?;
    f.write(OWNER, json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "spec":{"title":"Specification","role":"requirement","root":true,"parents":[]},
        "criterion":{"title":"Criterion","role":"ac","parents":["spec"]},
        "work":{"title":"Work","role":"task","parents":["criterion"],"parent_gate":"ready"}
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
    f.task(PLANNER, "work:t2", json!({"op":"amend_task","title":"t2","contract":"Implement the agreed mechanism with a new fix","projects":[],"reason":"Amend the contract"})).await?;
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
        f.write(OWNER, json!({"op":"member","id":actor,"role":"agent"}))
            .await?;
    }
    let schema = store::tree_schema(&f.pool, &f.ws, OWNER, None).await?;
    f.write(OWNER, json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure planning roles","types":{
        "story":{"title":"Story","role":"requirement","root":true,"parents":[]},
        "rule":{"title":"Rule","role":"ac","parents":["story"]},
        "job":{"title":"Job","role":"task","parents":["rule"],"parent_gate":"ready"}
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
