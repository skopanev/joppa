//! A group change keeps grey criterion A below group G and green criterion D in sibling
//! group H when a real server restarts and rebuilds the workspace from the journal.
use anyhow::{Result, ensure};
use joppa::store;
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
const A: &str = "tests/auth/login.test.ts > login > opens";
const B: &str = "tests/auth/login.test.ts > login > refuses";
const OWNER: &str = "owner";
const AGENT: &str = "agent";

struct Fixture {
    pool: deadpool_postgres::Pool,
    ws: String,
    http: reqwest::Client,
    root: String,
    agent: String,
    owner: String,
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
    async fn count(&self, statement: &str) -> Result<i64> {
        Ok(self
            .pool
            .get()
            .await?
            .query_one(statement, &[&self.ws])
            .await?
            .get(0))
    }
    /// The GET /tests reply for one criterion.
    async fn tests(&self, ac: &str) -> Result<Value> {
        let response = self
            .http
            .get(format!(
                "{}/api/workspaces/{}/tests?ac={ac}",
                self.root, self.ws
            ))
            .bearer_auth(&self.agent)
            .send()
            .await?;
        let code = response.status();
        let body: Value = response.json().await?;
        ensure!(code.is_success(), "GET /tests?ac={ac}: {code} {body}");
        Ok(body)
    }
    /// Assert the status of A and D at one step and return both replies.
    async fn expect(&self, step: &str, a: &str, d: &str) -> Result<(Value, Value)> {
        let (ra, rd) = (
            self.tests("criterion:a").await?,
            self.tests("criterion:d").await?,
        );
        ensure!(ra["status"] == a, "{step}: A must be {a}: {ra}");
        ensure!(rd["status"] == d, "{step}: D must be {d}: {rd}");
        Ok((ra, rd))
    }
    async fn post(&self, path: &str, token: &str, body: Value) -> Result<Value> {
        let response = self
            .http
            .post(format!("{}/api/workspaces/{}/{path}", self.root, self.ws))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;
        let code = response.status();
        let reply: Value = response.json().await?;
        ensure!(code.is_success(), "POST {path}: {code} {reply}");
        Ok(reply)
    }
}

// check-list-frozen-ac5
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn grey_and_green_criteria_keep_their_status_across_a_restart_that_rebuilds_the_journal()
-> Result<()> {
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
    let dir = std::env::temp_dir().join(format!(
        "joppa-grey-restart-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir)?;
    let (agent, owner) = (
        uuid::Uuid::new_v4().simple().to_string(),
        uuid::Uuid::new_v4().simple().to_string(),
    );
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({ agent.clone(): AGENT, owner.clone(): OWNER }).to_string(),
    )?;
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let f = Fixture {
        pool,
        ws: format!("grey-restart-{}", uuid::Uuid::new_v4().simple()),
        http: reqwest::Client::new(),
        root: format!("http://127.0.0.1:{port}"),
        agent,
        owner,
    };

    // Step 1: custom type names for all three roles, G with A and sibling H with D.
    store::execute_input(
        &f.pool,
        &f.ws,
        OWNER,
        "workspace",
        &json!({"op":"workspace","title":"Grey states across a restart"}),
    )
    .await?;
    f.write(OWNER, json!({"op":"member","id":AGENT,"role":"agent"}))
        .await?;
    let schema = store::tree_schema(&f.pool, &f.ws, OWNER, None).await?;
    f.write(OWNER, json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"reason":"Configure groups","types":{
        "area":{"title":"Area","role":"group","root":true,"parents":["area"]},
        "spec":{"title":"Specification","role":"requirement","parents":["area"]},
        "criterion":{"title":"Criterion","role":"ac","parents":["spec"]}
    }})).await?;
    for (area, spec, criterion) in [("g", "r", "a"), ("h", "rh", "d")] {
        f.write(OWNER, json!({"op":"save_object","kind":"area","id":area,"revision":0,"parent":null,"data":{"title":area.to_uppercase(),"body":"Group scope","owner":OWNER},"reason":"Create the group"})).await?;
        f.write(OWNER, json!({"op":"save_object","kind":"spec","id":spec,"revision":0,"parent":format!("area:{area}"),"data":{"title":spec,"body":"Implement the rule","owner":OWNER},"reason":"Create the spec"})).await?;
        f.write(OWNER, json!({"op":"save_object","kind":"criterion","id":criterion,"revision":0,"parent":format!("spec:{spec}"),"data":{"title":criterion,"owner":OWNER},"reason":"Create the criterion"})).await?;
        f.write(AGENT, json!({"op":"set_ac_tests","target":format!("criterion:{criterion}"),"tests":[A,B],"reason":"Bind the tests"})).await?;
    }
    let server = start(&url, &tokens, port).await?;
    let junit = r#"<testsuites><testsuite name="tests/auth/login.test.ts"><testsuite name="login"><testcase name="opens" /><testcase name="refuses" /></testsuite></testsuite></testsuites>"#;
    f.post(
        "reports",
        &f.agent,
        json!({"stamp":{"commit":"c1c1c1c","order":1,"branch":"master","default_branch":true,"dirty":false,"full":true},"junit":junit}),
    )
    .await?;
    f.expect("step 1, after the clean full report", "green", "green")
        .await?;

    // Step 2: the owner changes only the text of G.
    let g = store::read_objects(
        &f.pool,
        &f.ws,
        OWNER,
        &store::ObjectScope {
            address: Some("area:g"),
            ..Default::default()
        },
    )
    .await?["current_revision"]
        .clone();
    let command = json!({"op":"save_object","kind":"area","id":"g","revision":g,"parent":null,"data":{"title":"G","body":"Changed group scope","owner":OWNER},"links":[],"reason":"Change the group"});
    f.post(
        "commands",
        &f.owner,
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
    )
    .await?;
    let (a, d) = f
        .expect("step 2, after the change of G", "grey", "green")
        .await?;
    for (name, reply) in [("A", &a), ("D", &d)] {
        ensure!(
            reply["tests"].as_array().map(Vec::len) == Some(2) && reply["tests_revision"].is_u64(),
            "step 2: {name} must keep its test list and tests_revision: {reply}"
        );
    }
    store::check_journal(&f.pool).await?;

    // Step 3: stop the server; remove the projections, keep the journal and reports.
    drop(server);
    let events = "SELECT count(*) FROM joppa.events WHERE workspace=$1";
    let reports = "SELECT count(*) FROM joppa.test_reports WHERE workspace=$1";
    let results = "SELECT count(*) FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1";
    let kept = (
        f.count(events).await?,
        f.count(reports).await?,
        f.count(results).await?,
    );
    ensure!(
        kept.0 > 0 && kept.1 == 1 && kept.2 > 0,
        "step 3: journal and report rows before the delete: {kept:?}"
    );
    {
        let conn = f.pool.get().await?;
        for table in TABLES {
            let statement = format!("DELETE FROM joppa.{table} WHERE workspace=$1");
            conn.execute(&statement, &[&f.ws]).await?;
        }
        conn.execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&f.ws])
            .await?;
    }
    for table in TABLES.iter().chain(&["board_cache"]) {
        let rows = f
            .count(&format!(
                "SELECT count(*) FROM joppa.{table} WHERE workspace=$1"
            ))
            .await?;
        ensure!(
            rows == 0,
            "step 3: {table} keeps {rows} rows before startup"
        );
    }
    let after_delete = (
        f.count(events).await?,
        f.count(reports).await?,
        f.count(results).await?,
    );
    ensure!(
        after_delete == kept,
        "step 3: the delete changed journal or report rows: {kept:?} -> {after_delete:?}"
    );

    // Step 4: a new server on the same database rebuilds the workspace at startup.
    let server = start(&url, &tokens, port).await?;
    let rebuilt = f
        .count("SELECT count(*) FROM joppa.board_cache b WHERE b.workspace=$1 AND b.position=(SELECT MAX(e.position) FROM joppa.events e WHERE e.workspace=b.workspace)")
        .await?;
    ensure!(
        rebuilt == 1,
        "step 4: startup did not rebuild board_cache at the last event position"
    );
    let boundaries = f
        .count("SELECT count(*) FROM joppa.state_entries WHERE workspace=$1 AND section='criterion_boundaries' AND key='criterion:a'")
        .await?;
    ensure!(
        boundaries == 1,
        "step 4: startup did not rebuild the boundary of A"
    );
    let after = (f.tests("criterion:a").await?, f.tests("criterion:d").await?);
    drop(server);
    // Report a recovery difference with both replies before the statuses are checked.
    for (name, old, new) in [("A", &a, &after.0), ("D", &d, &after.1)] {
        ensure!(
            old == new,
            "step 4: {name} before the restart {old}, after {new}"
        );
    }
    ensure!(
        after.0["status"] == "grey" && after.1["status"] == "green",
        "step 4: A {} and D {} after the restart",
        after.0,
        after.1
    );
    store::check_journal(&f.pool).await?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
