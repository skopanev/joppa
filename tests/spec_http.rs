//! Specification behaviour over HTTP and MCP: a real server, an owner token and a test DB.
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
    root: String,
    ws: String,
    owner: String,
    alex: String,
    maria: String,
    bot: String,
    skopanev: String,
    /// A signed-in account that is not a member of the workspace.
    eve: String,
    pool: deadpool_postgres::Pool,
    http: reqwest::Client,
}

/// Create a workspace with the shared spec setup, then run `extra` as (actor, command).
async fn seed(pool: &deadpool_postgres::Pool, ws: &str, extra: Vec<(&str, Value)>) -> Result<()> {
    let setup = [
        json!({"op":"workspace","title":"Spec over HTTP"}),
        json!({"op":"member","id":"alex","role":"human"}),
        json!({"op":"member","id":"maria","role":"human"}),
        json!({"op":"member","id":"bot","role":"agent"}),
        json!({"op":"consumer","id":"customer","title":"Customer","owner":"owner"}),
        json!({"op":"domain","id":"security","title":"Security","authority":"owner"}),
        json!({"op":"capability","id":"restrict","title":"Restrict access","domain":"security"}),
        json!({"op":"capability","id":"empty","title":"Nothing below","domain":"security"}),
        json!({"op":"revise","req":"key","expected_revision":0,"capability":"restrict","title":"Key rights","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"key-rights","text":"Only read and verify before launch"}]}),
    ]
    .into_iter()
    .map(|op| ("owner", op))
    .chain(extra);
    for (i, (actor, op)) in setup.enumerate() {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(pool, ws, actor, &format!("setup-{i}"), &op).await?;
    }
    Ok(())
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
    let ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    seed(&pool, &ws, vec![]).await?;
    let dir = std::env::temp_dir().join(format!("joppa-spec-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let owner = uuid::Uuid::new_v4().simple().to_string();
    let alex = uuid::Uuid::new_v4().simple().to_string();
    let maria = uuid::Uuid::new_v4().simple().to_string();
    let bot = uuid::Uuid::new_v4().simple().to_string();
    let skopanev = uuid::Uuid::new_v4().simple().to_string();
    let eve = uuid::Uuid::new_v4().simple().to_string();
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({ owner.clone(): "owner", alex.clone(): "alex", maria.clone(): "maria", bot.clone(): "bot", skopanev.clone(): "skopanev", eve.clone(): "eve" }).to_string(),
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
            return Ok(World {
                _server: server,
                root,
                ws,
                owner,
                alex,
                maria,
                bot,
                skopanev,
                eve,
                pool,
                http: reqwest::Client::new(),
            });
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}

impl World {
    async fn command(&self, command: Value) -> Result<Value> {
        self.command_as(&self.owner, command).await
    }
    async fn command_as(&self, token: &str, command: Value) -> Result<Value> {
        let reply = self
            .http
            .post(format!("{}/api/workspaces/{}/commands", self.root, self.ws))
            .bearer_auth(token)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
            .send()
            .await?;
        ensure!(
            reply.status().is_success(),
            "{command}: {}",
            reply.text().await?
        );
        Ok(reply.json().await?)
    }
    async fn get(&self, query: &str) -> Result<Value> {
        self.get_as(&self.owner, query).await
    }
    async fn get_as(&self, token: &str, query: &str) -> Result<Value> {
        let reply = self
            .http
            .get(format!(
                "{}/api/workspaces/{}/objects?{query}",
                self.root, self.ws
            ))
            .bearer_auth(token)
            .send()
            .await?;
        ensure!(
            reply.status().is_success(),
            "{query}: {}",
            reply.text().await?
        );
        Ok(reply.json().await?)
    }
    async fn note(
        &self,
        id: &str,
        revision: u32,
        title: &str,
        body: &str,
        links: Value,
    ) -> Result<()> {
        self.command(json!({"op":"save_object","kind":"note","id":id,"revision":revision,"parent":null,"data":{"title":title,"owner":"owner","body":body},"links":links,"reason":"Save"})).await?;
        Ok(())
    }
    async fn search(&self, query: &str) -> Result<Vec<String>> {
        let reply: Value = self
            .http
            .post(format!("{}/mcp", self.root))
            .bearer_auth(&self.owner)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_search","arguments":{"workspace":self.ws,"query":query}}}))
            .send()
            .await?
            .json()
            .await?;
        let found: Value = serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        let rows = found["results"]
            .as_array()
            .or(found["items"].as_array())
            .cloned()
            .unwrap_or_default();
        Ok(rows
            .iter()
            .filter_map(|r| Some(format!("{}:{}", r["kind"].as_str()?, r["id"].as_str()?)))
            .collect())
    }
    /// The first search result, only when it is an exact ID or address match.
    async fn first_exact(&self, query: &str) -> Result<Option<String>> {
        let reply: Value = self
            .http
            .post(format!("{}/mcp", self.root))
            .bearer_auth(&self.owner)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_search","arguments":{"workspace":self.ws,"query":query}}}))
            .send()
            .await?
            .json()
            .await?;
        let found: Value = serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        let first = &found["items"][0];
        Ok((first["exact_id"] == true).then(|| {
            format!(
                "{}:{}",
                first["kind"].as_str().unwrap_or(""),
                first["id"].as_str().unwrap_or("")
            )
        }))
    }
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn completing_a_task_without_a_report_keeps_it_unverified() -> Result<()> {
    // done-is-not-green-ac1
    let w = world().await?;
    let mcp = |name: &str, arguments: Value| {
        let w = &w;
        let name = name.to_owned();
        async move {
            let response: Value = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.bot)
                .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}))
                .send().await?.error_for_status()?.json().await?;
            ensure!(response["result"]["isError"] != true, "{response}");
            Ok::<Value, anyhow::Error>(serde_json::from_str(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("MCP result needs text"))?,
            )?)
        }
    };
    let tree = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = tree["schema"]["types"].clone();
    types["job"] = types["task"].clone();
    types["job"]["title"] = json!("Job");
    w.command(json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"types":types,"reason":"Check another configured work type"})).await?;
    w.command_as(&w.bot, json!({"op":"set_ac_tests","target":"ac:key-rights","tests":["tests/spec_http.rs > completing_a_task_without_a_report_keeps_it_unverified"],"reason":"Wait for real test evidence"})).await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    for kind in ["task", "job"] {
        let address = format!("{kind}:unverified");
        w.command(json!({"op":"save_object","kind":kind,"id":"unverified","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the specified work","owner":"owner","contract":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Check completion before any report"})).await?;
        w.command_as(&w.bot, json!({"op":"claim_task","id":address,"revision":1}))
            .await?;
        let claimed = mcp("joppa_objects", json!({"workspace":w.ws,"address":address})).await?;
        ensure!(
            claimed["process"]["status"] == "in_progress"
                && claimed["process"]["assignee"] == "bot",
            "{claimed}"
        );
        w.command_as(&w.bot, json!({"op":"complete_task","id":address,"revision":claimed["current_revision"],"evidence":"Implemented the contract; test evidence is still absent"})).await?;
        let http: Value = w
            .http
            .get(format!("{}/api/workspaces/{}/objects", w.root, w.ws))
            .bearer_auth(&w.bot)
            .query(&[("address", &address)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let tool = mcp("joppa_objects", json!({"workspace":w.ws,"address":address})).await?;
        ensure!(http == tool, "HTTP and MCP disagree: {http} / {tool}");
        ensure!(
            http["process"]["status"] == "done"
                && http["process"]["verification"] == "unverified"
                && http["document"]["actor"] == "bot",
            "{http}"
        );
        let http_tests: Value = w
            .http
            .get(format!("{}/api/workspaces/{}/tests", w.root, w.ws))
            .bearer_auth(&w.bot)
            .query(&[("ac", "ac:key-rights")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let tool_tests = mcp(
            "joppa_tests",
            json!({"workspace":w.ws,"ac":"ac:key-rights"}),
        )
        .await?;
        ensure!(
            http_tests == tool_tests && http_tests["status"] == "grey",
            "HTTP and MCP criterion must stay grey: {http_tests} / {tool_tests}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_done_task_under_a_green_criterion_shows_checks_passed() -> Result<()> {
    // done-is-not-green-ac3
    let w = world().await?;
    let mcp = |name: &str, arguments: Value| {
        let w = &w;
        let name = name.to_owned();
        async move {
            let response: Value = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.alex)
                .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}))
                .send().await?.error_for_status()?.json().await?;
            ensure!(response["result"]["isError"] != true, "{response}");
            Ok::<Value, anyhow::Error>(serde_json::from_str(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("MCP result needs text"))?,
            )?)
        }
    };
    let task = "task:checked";
    w.command_as(&w.bot, json!({"op":"set_ac_tests","target":"ac:key-rights","tests":["checks.rs > key rights"],"reason":"Bind the checked test"})).await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"checked","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the specified work","owner":"owner","contract":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Check completion under a green criterion"})).await?;
    w.command_as(&w.bot, json!({"op":"claim_task","id":task,"revision":1}))
        .await?;
    w.command_as(
        &w.bot,
        json!({"op":"complete_task","id":task,"revision":2,"evidence":"Implemented the contract"}),
    )
    .await?;
    let stamp = json!({"commit":"c3c3c3c","order":1,"branch":"master","default_branch":true,"dirty":false,"full":true});
    w.http
        .post(format!("{}/api/workspaces/{}/reports", w.root, w.ws))
        .bearer_auth(&w.bot)
        .json(&json!({"stamp":stamp,"junit":r#"<testsuites><testsuite name="checks.rs"><testcase name="key rights" /></testsuite></testsuites>"#}))
        .send()
        .await?
        .error_for_status()?;
    let object = mcp("joppa_objects", json!({"workspace":w.ws,"address":task})).await?;
    ensure!(
        object["process"]["status"] == "done"
            && object["process"]["verification"] == "pass"
            && object["signal"]["tone"] == "green"
            && object["signal"]["label"] == "Work complete · Checks passed",
        "{object}"
    );
    let tests = mcp("joppa_tests", json!({"workspace":w.ws,"task":task})).await?;
    ensure!(
        tests["verification"] == "passed" && tests["criterion"]["status"] == "green",
        "{tests}"
    );
    let list = mcp(
        "joppa_objects",
        json!({"workspace":w.ws,"parent":"ac:key-rights"}),
    )
    .await?;
    let row = list["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["address"] == task))
        .ok_or_else(|| anyhow::anyhow!("the list needs the task: {list}"))?;
    ensure!(
        row["process"]["status"] == "done"
            && row["process"]["verification"] == "pass"
            && row["signal"]["tone"] == "green"
            && row["signal"]["label"] == "Work complete · Checks passed",
        "{row}"
    );
    // An unreadable criterion status keeps the bound-run verdict; T has no bound run.
    let broken = w.pool.get().await?.execute(
        "UPDATE joppa.state_entries SET value='{\"revisions\":\"unreadable\"}'::jsonb WHERE workspace=$1 AND section='ac_tests' AND key='ac:key-rights'",
        &[&w.ws],
    ).await?;
    ensure!(broken == 1, "the saved list must exist");
    let object = mcp("joppa_objects", json!({"workspace":w.ws,"address":task})).await?;
    let list = mcp(
        "joppa_objects",
        json!({"workspace":w.ws,"parent":"ac:key-rights"}),
    )
    .await?;
    let row = list["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["address"] == task))
        .ok_or_else(|| anyhow::anyhow!("the list needs the task: {list}"))?;
    for read in [&object, row] {
        ensure!(
            read["process"]["status"] == "done"
                && read["process"]["verification"] == "unverified"
                && read["signal"]["tone"] == "grey"
                && read["signal"]["label"] == "Work complete · Unverified",
            "{read}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_unreadable_task_criterion_keeps_a_mixed_list_readable() -> Result<()> {
    // done-is-not-green-ac3: the bound-run verdict stays when the criterion status cannot be read.
    let w = world().await?;
    w.command(json!({"op":"revise","req":"aaa","expected_revision":0,"capability":"restrict","title":"Other rule","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"aaa-rule","text":"Keep the other rule"}]})).await?;
    w.command_as(&w.bot, json!({"op":"set_ac_tests","target":"ac:aaa-rule","tests":["checks.rs > other rule"],"reason":"Bind the checked test"})).await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"aaa","revision":1,"ac":"aaa-rule","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"checked","revision":0,"parent":"ac:aaa-rule","data":{"title":"Implement the other rule","owner":"owner","contract":"Implement the other rule and record the result","purpose":"implementation","projects":[]},"reason":"Check completion under an unreadable criterion"})).await?;
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"task:checked","revision":1}),
    )
    .await?;
    w.command_as(&w.bot, json!({"op":"complete_task","id":"task:checked","revision":2,"evidence":"Implemented the contract"})).await?;
    let all = w.get("parent=*&limit=100").await?;
    let addresses: Vec<&str> = all["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["address"].as_str())
        .collect();
    let at = addresses
        .iter()
        .position(|a| *a == "requirement:key")
        .ok_or_else(|| anyhow::anyhow!("{all}"))?;
    ensure!(
        at > 0 && addresses[at - 1] == "requirement:aaa",
        "{addresses:?}"
    );
    let broken = w.pool.get().await?.execute(
        "UPDATE joppa.state_entries SET value='{\"revisions\":\"unreadable\"}'::jsonb WHERE workspace=$1 AND section='ac_tests' AND key='ac:aaa-rule'",
        &[&w.ws],
    ).await?;
    ensure!(broken == 1, "the saved list must exist");
    // The page holds requirement:key with its summary and the Task, but not requirement:aaa.
    let page = w
        .get(&format!(
            "parent=*&limit=2&cursor={}:requirement:aaa",
            all["position"]
        ))
        .await?;
    let (summary, task) = (&page["items"][0], &page["items"][1]);
    ensure!(
        summary["address"] == "requirement:key"
            && summary["verification_summary"]["total"] == 1
            && task["address"] == "task:checked"
            && task["process"]["status"] == "done"
            && task["process"]["verification"] == "unverified"
            && task["signal"]["tone"] == "grey"
            && task["signal"]["label"] == "Work complete · Unverified",
        "{page}"
    );
    Ok(())
}

// [ac:completion-record-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn completing_a_task_stores_exact_evidence_and_bot_event_actor() -> Result<()> {
    let w = world().await?;
    let read = |address: &str| {
        let w = &w;
        let arguments = json!({"workspace":w.ws,"address":address});
        async move {
            let result = mcp_call(w, &w.bot, "joppa_objects", arguments).await?;
            ensure!(result["isError"] != true, "{result}");
            Ok::<Value, anyhow::Error>(serde_json::from_str(
                result["content"][0]["text"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("MCP result needs text"))?,
            )?)
        }
    };
    let address = "task:recorded";
    let evidence = "commit abc123 on master";
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"recorded","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the specified work","owner":"owner","contract":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Check the completion record"})).await?;
    w.command_as(&w.bot, json!({"op":"claim_task","id":address,"revision":1}))
        .await?;
    let claimed = read(address).await?;
    ensure!(
        claimed["process"]["status"] == "in_progress" && claimed["process"]["assignee"] == "bot",
        "{claimed}"
    );
    w.command_as(&w.bot, json!({"op":"complete_task","id":address,"revision":claimed["current_revision"],"evidence":evidence})).await?;
    let tool = read(address).await?;
    ensure!(
        tool["process"]["status"] == "done" && tool["process"]["done"] == evidence,
        "{tool}"
    );
    let http: Value = w
        .http
        .get(format!("{}/api/workspaces/{}/objects", w.root, w.ws))
        .bearer_auth(&w.bot)
        .query(&[("address", address)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(http == tool, "HTTP and MCP disagree: {http} / {tool}");
    let events = w
        .pool
        .get()
        .await?
        .query(
            "SELECT actor FROM joppa.events WHERE workspace=$1 AND command->>'op'='complete_task' AND command->>'id'=$2",
            &[&w.ws, &address],
        )
        .await?;
    let actors: Vec<String> = events.iter().map(|row| row.get(0)).collect();
    ensure!(actors == ["bot"], "one bot completion event: {actors:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_account_without_membership_cannot_upload_a_file() -> Result<()> {
    use sha2::{Digest, Sha256};
    // files-keep-versions-ac2
    let w = world().await?;
    w.note(
        "file-target",
        0,
        "Private files",
        "Workspace members only",
        json!([]),
    )
    .await?;
    let bytes = format!("Unaccepted file {}", uuid::Uuid::new_v4());
    let hash = format!("{:x}", Sha256::digest(bytes.as_bytes()));
    let before = w.get("address=note:file-target").await?;
    ensure!(before["files"] == json!([]), "{before}");
    let mut replies = Vec::new();
    for workspace in [
        w.ws.clone(),
        format!("absent-{}", uuid::Uuid::new_v4().simple()),
    ] {
        let response = w
            .http
            .post(format!("{}/api/workspaces/{workspace}/blobs", w.root))
            .bearer_auth(&w.eve)
            .body(bytes.clone())
            .send()
            .await?;
        let status = response.status().as_u16();
        let body: Value = response.json().await?;
        ensure!(
            status == 404 && body == json!({"error":"Workspace not found"}),
            "{status}: {body}"
        );
        replies.push((status, body));
        let after = w.get("address=note:file-target").await?;
        ensure!(
            after == before,
            "A refused upload changed the object: {after}"
        );
        let blobs: i64 = w
            .pool
            .get()
            .await?
            .query_one("SELECT count(*) FROM joppa.blobs WHERE hash=$1", &[&hash])
            .await?
            .get(0);
        ensure!(blobs == 0, "A refused upload stored file content");
    }
    ensure!(
        replies[0] == replies[1],
        "Upload replies expose workspace existence: {replies:?}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_previous_assignee_cannot_complete_returned_work() -> Result<()> {
    // old-agent-locked-out-ac1
    let w = world().await?;
    let first = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let second = store::create_agent_token(&w.pool, &w.ws, "owner", "bot2", None).await?;
    let bot = first["token"].as_str().unwrap();
    let bot2 = second["token"].as_str().unwrap();
    w.command_as(bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"returned","revision":0,"parent":"ac:key-rights","data":{"title":"Complete the defined work","owner":"owner","contract":"Implement the key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Create work for transfer"})).await?;
    w.command_as(
        bot,
        json!({"op":"claim_task","id":"task:returned","revision":1}),
    )
    .await?;
    w.command_as(bot, json!({"op":"release_task","id":"task:returned","revision":2,"reason":"Return the unfinished work"})).await?;
    w.command_as(
        bot2,
        json!({"op":"claim_task","id":"task:returned","revision":3}),
    )
    .await?;
    let claimed = w.get("address=task:returned").await?;
    ensure!(claimed["current_revision"] == 4, "{claimed}");
    ensure!(claimed["process"]["assignee"] == "bot2", "{claimed}");
    let conn = w.pool.get().await?;
    let before: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    let request_id = uuid::Uuid::new_v4().to_string();
    let complete = json!({"op":"complete_task","id":"task:returned","revision":4,"evidence":"Implemented the contract and recorded checks"});
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(bot)
        .json(&json!({"request_id":request_id,"command":complete}))
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::CONFLICT,
        "{}",
        response.status()
    );
    let response: Value = response.json().await?;
    ensure!(
        response["error"] == "Task is not claimed unfinished work",
        "{response}"
    );
    let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":request_id,"command":complete}}})).send().await?;
    ensure!(response.status().is_success(), "{}", response.status());
    let response: Value = response.json().await?;
    ensure!(
        response["result"]["isError"] == true
            && response["result"]["content"][0]["text"] == "Task is not claimed unfinished work",
        "{response}"
    );
    let after: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(before == after, "The refused completion appended an event");
    let receipts: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2",
            &[&w.ws, &request_id],
        )
        .await?
        .get(0);
    ensure!(
        receipts == 0,
        "The refused completion consumed its request ID"
    );
    let unchanged = w.get("address=task:returned").await?;
    ensure!(
        unchanged["document"] == claimed["document"] && unchanged["process"] == claimed["process"],
        "The refused completion changed the task: {unchanged}"
    );
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(bot2)
        .json(&json!({"request_id":request_id,"command":complete}))
        .send()
        .await?;
    ensure!(response.status().is_success(), "{}", response.text().await?);
    let done = w.get("address=task:returned").await?;
    ensure!(
        done["current_revision"] == 5
            && done["document"]["actor"] == "bot2"
            && done["process"]["done"].is_string(),
        "{done}"
    );
    w.command(json!({"op":"save_object","kind":"task","id":"owner-completion","revision":0,"parent":"ac:key-rights","data":{"title":"Check completion authority","owner":"owner","contract":"Implement the same key rights contract","purpose":"implementation","projects":[]},"reason":"Check completion by the current holder"})).await?;
    w.command_as(
        bot2,
        json!({"op":"claim_task","id":"task:owner-completion","revision":1}),
    )
    .await?;
    let owned = w.get("address=task:owner-completion").await?;
    let complete = json!({"op":"complete_task","id":"task:owner-completion","revision":2,"evidence":"Record the completed work"});
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":complete}))
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::CONFLICT,
        "{}",
        response.status()
    );
    let response: Value = response.json().await?;
    ensure!(
        response["error"] == "Task is not claimed unfinished work",
        "{response}"
    );
    let unchanged = w.get("address=task:owner-completion").await?;
    ensure!(
        unchanged["document"] == owned["document"] && unchanged["process"] == owned["process"],
        "The refused owner completion changed the Task"
    );
    w.command_as(bot2, complete).await?;
    let done = w.get("address=task:owner-completion").await?;
    ensure!(
        done["document"]["actor"] == "bot2" && done["process"]["done"].is_string(),
        "{done}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_released_holder_cannot_complete_open_or_reclaimed_work() -> Result<()> {
    // old-agent-locked-out-ac1
    // A Task owner must be a human member, so P is alex in the variant where P owns the Task.
    for (holder, owner_releases) in [
        ("bot", false),
        ("bot", true),
        ("alex", false),
        ("alex", true),
    ] {
        released_holder_cannot_complete(holder, owner_releases)
            .await
            .map_err(|e| e.context(format!("holder {holder}, owner releases: {owner_releases}")))?;
    }
    Ok(())
}

/// P (`holder`) claims T, then P or the workspace owner releases it; P's completion is refused
/// while T is open and after Q claims it, then Q completes T. Alex as P owns the Task document.
async fn released_holder_cannot_complete(holder: &str, owner_releases: bool) -> Result<()> {
    let w = world().await?;
    let first = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let second = store::create_agent_token(&w.pool, &w.ws, "owner", "bot2", None).await?;
    let bot = first["token"].as_str().unwrap();
    let bot2 = second["token"].as_str().unwrap();
    let (p, document_owner) = if holder == "alex" {
        (w.alex.as_str(), "alex")
    } else {
        (bot, "owner")
    };
    w.command_as(bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"released","revision":0,"parent":"ac:key-rights","data":{"title":"Complete the defined work","owner":document_owner,"contract":"Implement the key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Create work for release"})).await?;
    w.command_as(
        p,
        json!({"op":"claim_task","id":"task:released","revision":1}),
    )
    .await?;
    let claimed = w.get("address=task:released").await?;
    ensure!(
        claimed["process"]["assignee"] == holder
            && claimed["document"]["data"]["owner"] == document_owner,
        "{claimed}"
    );
    let release_token = if owner_releases { &w.owner } else { p };
    w.command_as(release_token, json!({"op":"release_task","id":"task:released","revision":2,"reason":"Return the unfinished work"})).await?;
    for mcp in [false, true] {
        let open = refused_completion(&w, p, "task:released", mcp).await?;
        ensure!(
            open["process"]["status"] == "open"
                && open["process"]["assignee"].is_null()
                && open["process"]["done"].is_null(),
            "{open}"
        );
    }
    w.command_as(
        bot2,
        json!({"op":"claim_task","id":"task:released","revision":3}),
    )
    .await?;
    for mcp in [false, true] {
        let claimed = refused_completion(&w, p, "task:released", mcp).await?;
        ensure!(
            claimed["process"]["status"] == "in_progress"
                && claimed["process"]["assignee"] == "bot2"
                && claimed["process"]["done"].is_null(),
            "{claimed}"
        );
    }
    w.command_as(bot2, json!({"op":"complete_task","id":"task:released","revision":4,"evidence":"Implemented the contract and recorded checks"})).await?;
    let done = w.get("address=task:released").await?;
    ensure!(
        done["current_revision"] == 5
            && done["document"]["actor"] == "bot2"
            && done["process"]["done"].is_string(),
        "{done}"
    );
    Ok(())
}

/// One completion by `token` over HTTP or MCP at the current revision; checks the exact refusal
/// and that the Task and every stored row, including the journal position, are unchanged.
/// Returns the Task as read before the attempt.
async fn refused_completion(w: &World, token: &str, address: &str, mcp: bool) -> Result<Value> {
    let task = w.get(&format!("address={address}")).await?;
    let saved = saved_rows(w).await?;
    let complete = json!({"op":"complete_task","id":address,"revision":task["current_revision"],"evidence":"Implemented the contract and recorded checks"});
    let request = json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":complete});
    let error = if mcp {
        let result = mcp_call(
            w,
            token,
            "joppa_command",
            json!({"workspace":w.ws,"request_id":request["request_id"],"command":complete}),
        )
        .await?;
        ensure!(result["isError"] == true, "{result}");
        result["content"][0]["text"].clone()
    } else {
        let response = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(token)
            .json(&request)
            .send()
            .await?;
        ensure!(
            response.status() == reqwest::StatusCode::CONFLICT,
            "{}",
            response.status()
        );
        response.json::<Value>().await?["error"].clone()
    };
    ensure!(error == "Task is not claimed unfinished work", "{error}");
    let unchanged = w.get(&format!("address={address}")).await?;
    ensure!(
        unchanged["current_revision"] == task["current_revision"]
            && unchanged["process"]["verification"] == task["process"]["verification"]
            && unchanged["process"] == task["process"]
            && unchanged["document"] == task["document"],
        "The refused completion changed the Task: {unchanged}"
    );
    let rows = saved_rows(w).await?;
    ensure!(
        rows["position"] == saved["position"] && rows == saved,
        "The refused completion changed stored rows"
    );
    Ok(task)
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn blank_completion_evidence_is_refused_over_http_and_mcp() -> Result<()> {
    // completion-record-ac3
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    w.command_as(bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"blank-evidence","revision":0,"parent":"ac:key-rights","data":{"title":"Complete the defined work","owner":"owner","contract":"Implement the key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Check blank completion evidence"})).await?;
    w.command_as(
        bot,
        json!({"op":"claim_task","id":"task:blank-evidence","revision":1}),
    )
    .await?;
    let claimed = w.get("address=task:blank-evidence").await?;
    ensure!(
        claimed["process"]["status"] == "in_progress"
            && claimed["process"]["assignee"] == "bot"
            && claimed["process"]["done"].is_null(),
        "{claimed}"
    );
    let conn = w.pool.get().await?;
    let before: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    let request_id = uuid::Uuid::new_v4().to_string();
    let complete = json!({"op":"complete_task","id":"task:blank-evidence","revision":claimed["current_revision"],"evidence":"  "});
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(bot)
        .json(&json!({"request_id":request_id,"command":complete}))
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::CONFLICT,
        "{}",
        response.status()
    );
    let response: Value = response.json().await?;
    ensure!(response["error"] == "text must not be empty", "{response}");
    let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":request_id,"command":complete}}})).send().await?;
    ensure!(response.status().is_success(), "{}", response.status());
    let response: Value = response.json().await?;
    ensure!(
        response["result"]["isError"] == true
            && response["result"]["content"][0]["text"] == "text must not be empty",
        "{response}"
    );
    let unchanged = w.get("address=task:blank-evidence").await?;
    ensure!(
        unchanged["process"]["status"] == "in_progress"
            && unchanged["process"]["assignee"] == "bot"
            && unchanged["process"]["done"].is_null()
            && unchanged["current_revision"] == claimed["current_revision"]
            && unchanged["process"] == claimed["process"],
        "The refused completion changed the Task: {unchanged}"
    );
    let after: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(before == after, "The refused completion appended an event");
    let receipts: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2",
            &[&w.ws, &request_id],
        )
        .await?
        .get(0);
    ensure!(
        receipts == 0,
        "The refused completion consumed its request ID"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_cannot_set_task_verification_by_hand() -> Result<()> {
    // done-is-not-green-ac2
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["job"] = types["task"].clone();
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check verification with a configured task type"})).await?;
    w.command_as(bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let conn = w.pool.get().await?;
    for kind in ["task", "job"] {
        w.command(json!({"op":"save_object","kind":kind,"id":"report-only","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the report contract","owner":"owner","contract":"Implement the key rights and record checks","purpose":"implementation","projects":[]},"reason":"Create work for the verification guard"})).await?;
        let address = format!("{kind}:report-only");
        w.command_as(bot, json!({"op":"claim_task","id":address,"revision":1}))
            .await?;
        let claimed = w.get(&format!("address={address}")).await?;
        ensure!(
            claimed["process"]["assignee"] == "bot"
                && claimed["process"]["verification"] == "unverified",
            "{claimed}"
        );
        let before: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
                &[&w.ws],
            )
            .await?
            .get(0);
        let request_id = uuid::Uuid::new_v4().to_string();
        for verification in [json!("passing"), Value::Null] {
            let mut data = claimed["document"]["data"].clone();
            data["verification"] = verification;
            let command = json!({"op":"save_object","kind":kind,"id":"report-only","revision":claimed["current_revision"],"parent":"ac:key-rights","data":data,"reason":"Attempt to set verification by hand"});
            let response = w
                .http
                .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
                .bearer_auth(bot)
                .json(&json!({"request_id":request_id,"command":command}))
                .send()
                .await?;
            ensure!(
                response.status() == reqwest::StatusCode::CONFLICT,
                "{}",
                response.status()
            );
            let response: Value = response.json().await?;
            ensure!(
                response["error"] == "verification comes from test reports",
                "{response}"
            );
            let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":request_id,"command":command}}})).send().await?;
            ensure!(response.status().is_success(), "{}", response.status());
            let response: Value = response.json().await?;
            ensure!(
                response["result"]["isError"] == true
                    && response["result"]["content"][0]["text"]
                        == "verification comes from test reports",
                "{response}"
            );
        }
        let after: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
                &[&w.ws],
            )
            .await?
            .get(0);
        ensure!(
            before == after,
            "The refused verification write appended an event"
        );
        let receipts: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2",
                &[&w.ws, &request_id],
            )
            .await?
            .get(0);
        ensure!(
            receipts == 0,
            "The refused verification write consumed its request ID"
        );
        let unchanged = w.get(&format!("address={address}")).await?;
        ensure!(
            unchanged["document"] == claimed["document"]
                && unchanged["process"] == claimed["process"],
            "The refused verification write changed the task: {unchanged}"
        );
        let mut data = claimed["document"]["data"].clone();
        data["context"] = json!({"verification":"passing"});
        let valid = json!({"op":"save_object","kind":kind,"id":"report-only","revision":claimed["current_revision"],"parent":"ac:key-rights","data":data,"reason":"Keep a nested business value"});
        let response = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(bot)
            .json(&json!({"request_id":request_id,"command":valid}))
            .send()
            .await?;
        ensure!(response.status().is_success(), "{}", response.text().await?);
        let updated = w.get(&format!("address={address}")).await?;
        ensure!(
            updated["document"]["data"]["context"]["verification"] == "passing"
                && updated["process"]["verification"] == "unverified",
            "{updated}"
        );
        w.command_as(bot, json!({"op":"release_task","id":address,"revision":updated["current_revision"],"reason":"Return the tested work"})).await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_cannot_supply_the_actor_of_a_write() -> Result<()> {
    // no-shared-credentials-ac1
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let error = "actor comes from the identity, not the request";
    let command = json!({"op":"save_object","kind":"note","id":"identity-guard","revision":0,"parent":null,"data":{"title":"Identity guard","owner":"owner","body":"Business context","actor":"business actor","source":{"actor":"skopanev"}},"reason":"Keep the authenticated author"});
    let request_id = uuid::Uuid::new_v4().to_string();
    let conn = w.pool.get().await?;
    let before: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    for claimed in [json!("skopanev"), Value::Null] {
        for in_command in [false, true] {
            let mut input = json!({"request_id":request_id,"command":command});
            if in_command {
                input["command"]["actor"] = claimed.clone();
            } else {
                input["actor"] = claimed.clone();
            }
            let response = w
                .http
                .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
                .bearer_auth(bot)
                .json(&input)
                .send()
                .await?;
            ensure!(
                response.status() == reqwest::StatusCode::UNPROCESSABLE_ENTITY,
                "{}: {}",
                response.status(),
                response.text().await?
            );
            let response: Value = response.json().await?;
            ensure!(response["error"] == error, "{response}");
        }
    }
    let mut nested = command.clone();
    nested["actor"] = json!("skopanev");
    let cases = [
        (
            "joppa_command",
            json!({"workspace":w.ws,"request_id":request_id,"actor":"skopanev","command":command}),
        ),
        (
            "joppa_command",
            json!({"workspace":w.ws,"request_id":request_id,"command":nested}),
        ),
        (
            "joppa_batch",
            json!({"workspace":w.ws,"request_id":request_id,"actor":"skopanev","operations":[{"command":command}]}),
        ),
        (
            "joppa_batch",
            json!({"workspace":w.ws,"request_id":request_id,"operations":[{"command":command},{"command":nested}]}),
        ),
        (
            "joppa_batch",
            json!({"workspace":w.ws,"request_id":request_id,"operations":[{"actor":"skopanev","command":command}]}),
        ),
    ];
    for (name, arguments) in cases {
        let expected = if name == "joppa_batch"
            && arguments["operations"][1]["command"].get("actor").is_some()
        {
            format!("operation 2: {error}")
        } else {
            error.to_owned()
        };
        let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}})).send().await?;
        ensure!(response.status().is_success(), "{}", response.status());
        let response: Value = response.json().await?;
        ensure!(
            response["result"]["isError"] == true
                && response["result"]["content"][0]["text"] == expected,
            "{response}"
        );
    }
    let after: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(before == after, "A refused write appended an event");
    let impersonated: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1 AND actor='skopanev'",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(impersonated == 0, "The bot token wrote as a person");
    let receipts: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2",
            &[&w.ws, &request_id],
        )
        .await?
        .get(0);
    ensure!(receipts == 0, "A refused write consumed its request ID");
    let saved = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(bot)
        .json(&json!({"request_id":request_id,"command":command}))
        .send()
        .await?;
    ensure!(saved.status().is_success(), "{}", saved.text().await?);
    let read = w.get("address=note:identity-guard").await?;
    ensure!(read["document"]["actor"] == "bot", "{read}");
    ensure!(
        read["document"]["data"]["actor"] == "business actor"
            && read["document"]["data"]["source"]["actor"] == "skopanev",
        "{read}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_requirement_without_criteria_reads_as_no_criteria_over_mcp() -> Result<()> {
    // req-has-criteria-ac2
    async fn read(w: &World, bot: &str, arguments: Value) -> Result<Value> {
        let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_read","arguments":arguments}})).send().await?;
        ensure!(response.status().is_success(), "{}", response.status());
        let response: Value = response.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        Ok(serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?)
    }
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["spec"] = types["requirement"].clone();
    types["spec"]["parents"] = json!(["*"]);
    types["spec"]["root"] = json!(true);
    types["condition"] = types["ac"].clone();
    types["condition"]["parents"] = json!(["spec"]);
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Test configured specification and criterion names"})).await?;
    for (kind, parent, criterion) in [
        ("requirement", Some("capability:empty"), "ac"),
        ("spec", None, "condition"),
    ] {
        w.command(json!({"op":"save_object","kind":kind,"id":"empty","revision":0,"parent":parent,"data":{"title":"The empty specification","body":"Define the exact behavior before work","owner":"owner","consumers":["customer"]},"reason":"Create a specification without criteria"})).await?;
        let address = format!("{kind}:empty");
        let object = w.get(&format!("address={address}")).await?;
        let item_id = object["item_id"].clone();
        for arguments in [
            json!({"workspace":w.ws,"req":address}),
            json!({"item_id":item_id}),
        ] {
            let empty = read(&w, bot, arguments).await?;
            ensure!(empty["status"] == "no criteria", "{empty}");
            ensure!(empty["verification"] == "unverified", "{empty}");
            ensure!(
                empty["requirement"]["revisions"][0]["acs"]
                    .as_array()
                    .is_some_and(|acs| acs.is_empty()),
                "{empty}"
            );
            ensure!(
                empty["summary"]["accepted"] == false
                    && empty["summary"]["ready_for_acceptance"] == false,
                "{empty}"
            );
        }
        let history = read(
            &w,
            bot,
            json!({"workspace":w.ws,"req":address,"history":true}),
        )
        .await?;
        ensure!(
            history.get("status").is_none() && history["requirement"].get("revisions").is_none(),
            "Counts-only history included document status: {history}"
        );
        w.command(json!({"op":"save_object","kind":criterion,"id":format!("{kind}-condition"),"revision":0,"parent":address,"data":{"title":"The defined behavior holds","owner":"owner","description":"Check the agreed behavior","depends_on":[],"before_launch":false,"decision_required":false},"reason":"Add the first criterion"})).await?;
        let current = read(&w, bot, json!({"workspace":w.ws,"req":address})).await?;
        ensure!(
            current["status"] != "no criteria",
            "The current nonempty revision still has no criteria: {current}"
        );
        ensure!(
            current["requirement"]["revisions"][0]["acs"]
                .as_array()
                .is_some_and(|acs| acs.len() == 1),
            "{current}"
        );
        for arguments in [
            json!({"workspace":w.ws,"req":address,"revision":1}),
            json!({"item_id":item_id,"revision":1}),
        ] {
            let old = read(&w, bot, arguments).await?;
            ensure!(
                old["status"] == "no criteria" && old["verification"] == "unverified",
                "The saved empty revision lost its status: {old}"
            );
            ensure!(old["requirement"]["revisions"][0]["number"] == 1, "{old}");
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn returned_work_keeps_notes_and_partial_results_for_the_next_agent_after_holder_release()
-> Result<()> {
    // returned-work-kept-ac1
    returned_work_handover(false).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn returned_work_keeps_notes_and_partial_results_for_the_next_agent_after_owner_release()
-> Result<()> {
    // returned-work-kept-ac1
    returned_work_handover(true).await
}

/// One object read through MCP as `token`: the current revision, or exactly `revision`.
async fn object_as(w: &World, token: &str, address: &str, revision: Option<u32>) -> Result<Value> {
    let mut arguments = json!({"workspace":w.ws,"address":address});
    if let Some(revision) = revision {
        arguments["revision"] = json!(revision);
    }
    let result = mcp_call(w, token, "joppa_objects", arguments).await?;
    ensure!(result["isError"] != true, "{result}");
    Ok(serde_json::from_str(
        result["content"][0]["text"].as_str().unwrap_or("null"),
    )?)
}

/// P (bot) holds T with two notes and a partial run at def456. P or the workspace owner
/// releases T; Q (bot2) claims it and reads the same notes, commit and earlier revisions.
async fn returned_work_handover(owner_release: bool) -> Result<()> {
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot2", None).await?;
    let bot2 = token["token"].as_str().unwrap();
    let notes = [
        ("first", "The first boundary is clear"),
        ("second", "The second boundary still needs work"),
    ];
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"handover","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the key contract","owner":"owner","contract":"Implement the exact key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Create work for the handover"})).await?;
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"task:handover","revision":1}),
    )
    .await?;
    for (id, text) in notes {
        w.command_as(
            &w.bot,
            json!({"op":"note","target":"task:handover","id":id,"text":text}),
        )
        .await?;
    }
    w.command(json!({"op":"define_verification_scope","req":"key","revision":1,"subject_keys":["commit"],"scope":"The key rights","reason":"Record the exact partial commit"})).await?;
    w.command_as(&w.bot, json!({"op":"plan","req":"key","revision":1,"run":"partial","subject":{"commit":"def456"},"scope":"The key rights","discovery_complete":false,"checks":[{"id":"[key-rights] first-boundary","ac":"key-rights","method":"automated","expected":["The first boundary holds"],"tasks":[{"address":"task:handover","origin_revision":1}]},{"id":"[key-rights] second-boundary","ac":"key-rights","method":"automated","expected":["The second boundary holds"],"tasks":[{"address":"task:handover","origin_revision":1}]}]})).await?;
    let partial = json!({"check":"[key-rights] first-boundary","outcome":"pass","observed":["The first boundary holds"],"note":"Partial implementation at commit def456","link":"https://example.test/repository/commit/def456"});
    w.command_as(
        &w.bot,
        json!({"op":"result","req":"key","revision":1,"run":"partial","evidence":partial}),
    )
    .await?;

    let before = object_as(&w, bot2, "task:handover", None).await?;
    let n = before["current_revision"].as_u64().unwrap() as u32;
    let mut history = Vec::new();
    for revision in 1..=n {
        history
            .push(object_as(&w, bot2, "task:handover", Some(revision)).await?["document"].clone());
    }
    let (releaser, actor, reason) = if owner_release {
        (&w.owner, "owner", "The worker is unavailable")
    } else {
        (&w.bot, "bot", "Return the remaining boundary to the queue")
    };
    w.command_as(
        releaser,
        json!({"op":"release_task","id":"task:handover","revision":n,"reason":reason}),
    )
    .await?;
    let available = mcp_call(
        &w,
        bot2,
        "joppa_tasks",
        json!({"workspace":w.ws,"view":"available"}),
    )
    .await?;
    ensure!(available["isError"] != true, "{available}");
    let available: Value =
        serde_json::from_str(available["content"][0]["text"].as_str().unwrap_or("null"))?;
    ensure!(
        available["tasks"]
            .as_array()
            .is_some_and(|tasks| tasks.iter().any(|task| task["address"] == "task:handover")),
        "{available}"
    );
    w.command_as(
        bot2,
        json!({"op":"claim_task","id":"task:handover","revision":n + 1}),
    )
    .await?;

    let task = object_as(&w, bot2, "task:handover", None).await?;
    ensure!(task["process"]["assignee"] == "bot2", "{task}");
    ensure!(task["process"]["status"] == "in_progress", "{task}");
    ensure!(
        task["document"]["data"] == before["document"]["data"],
        "the handover changed the document: {task}"
    );
    let comments = &task["annotations"]["comments"];
    for (id, text) in notes {
        let note = &before["annotations"]["comments"][id];
        ensure!(
            comments[id] == *note,
            "note {id} changed: {note} became {}",
            comments[id]
        );
        ensure!(
            note["author"] == "bot" && note["text"] == text,
            "note {id} lost its author or text: {note}"
        );
    }
    ensure!(
        task["annotations"] == before["annotations"],
        "the handover changed the original notes: {task}"
    );

    ensure!(
        task["current_revision"] == n + 2,
        "expected revision {} after the release and claim: {task}",
        n + 2
    );
    for (revision, captured) in (1..=n).zip(&history) {
        let read = object_as(&w, bot2, "task:handover", Some(revision)).await?;
        ensure!(
            read["document"] == *captured,
            "revision {revision} changed: {captured} became {}",
            read["document"]
        );
    }
    let released = object_as(&w, bot2, "task:handover", Some(n + 1)).await?["document"].clone();
    ensure!(
        released["actor"] == actor
            && released["reason"] == reason
            && released["data"] == before["document"]["data"],
        "revision {} is not the release: {released}",
        n + 1
    );
    let claimed = object_as(&w, bot2, "task:handover", Some(n + 2)).await?["document"].clone();
    ensure!(
        claimed["actor"] == "bot2"
            && claimed["reason"] == "Task claimed"
            && claimed["data"] == before["document"]["data"],
        "revision {} is not the claim by bot2: {claimed}",
        n + 2
    );

    let evidence = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/board?level=process&id=task:handover",
            w.root, w.ws
        ))
        .bearer_auth(bot2)
        .send()
        .await?;
    ensure!(evidence.status().is_success(), "{}", evidence.text().await?);
    let evidence: Value = evidence.json().await?;
    let run = &evidence["runs"]["partial"];
    ensure!(
        run["subject"]["commit"] == "def456" && run["reporter"] == "bot",
        "{run}"
    );
    ensure!(
        run["finished"].is_null() && run["discovery_complete"] == false,
        "{run}"
    );
    ensure!(
        run["results"]["[key-rights] first-boundary"] == partial
            && run["results"]["[key-rights] second-boundary"].is_null(),
        "{run}"
    );
    ensure!(
        run["plan"][0]["tasks"][0]["address"] == "task:handover",
        "{run}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_comment_records_its_author_and_server_time() -> Result<()> {
    // comments-on-object-ac1
    let w = world().await?;
    let clock = w.pool.get().await?;
    let server_time = || async {
        let time: String = clock
            .query_one("SELECT clock_timestamp()::text", &[])
            .await?
            .get(0);
        Ok::<_, anyhow::Error>(
            chrono::DateTime::parse_from_str(&time, "%Y-%m-%d %H:%M:%S%.f%#z")?
                .with_timezone(&chrono::Utc),
        )
    };
    w.note("input", 0, "Input", "Context", json!([])).await?;
    for address in [
        "domain:security",
        "capability:restrict",
        "requirement:key",
        "ac:key-rights",
        "note:input",
    ] {
        let before = server_time().await?;
        let response = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(&w.alex)
            .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"note","target":address,"id":"n1","text":"Clarify the boundary"}}))
            .send()
            .await?;
        ensure!(response.status().is_success(), "{}", response.text().await?);
        let object = w.get(&format!("address={address}")).await?;
        let comment = &object["annotations"]["comments"]["n1"];
        ensure!(comment["author"] == "alex", "{comment}");
        ensure!(comment["actor_type"] == "person", "{comment}");
        ensure!(comment["actor"] == comment["author"], "{comment}");
        ensure!(comment["at"] == comment["created_at"], "{comment}");
        let created = chrono::DateTime::parse_from_str(
            comment["created_at"].as_str().unwrap_or(""),
            "%Y-%m-%d %H:%M:%S%.f%#z",
        )?
        .with_timezone(&chrono::Utc);
        let after = server_time().await?;
        ensure!(
            created >= before && created <= after,
            "{comment}; server interval {before} to {after}"
        );
        ensure!(
            (created - before).num_milliseconds() <= 5000,
            "comment time exceeds the five second write interval: {comment}"
        );
        let response = w
            .http
            .post(format!("{}/mcp", w.root))
            .bearer_auth(&w.alex)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":w.ws,"address":address}}}))
            .send()
            .await?;
        ensure!(response.status().is_success(), "{}", response.text().await?);
        let reply: Value = response.json().await?;
        let object: Value = serde_json::from_str(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        ensure!(
            object["annotations"]["comments"]["n1"] == *comment,
            "{reply}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_account_without_membership_cannot_comment_on_an_object() -> Result<()> {
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    let question = "Which fact does this record preserve?";
    types["record"] =
        json!({"title":"Record","role":"document","root":true,"parents":[],"question":question});
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Use a configured document type"})).await?;
    w.command(json!({"op":"save_object","kind":"record","id":"one","revision":0,"parent":null,"data":{"title":"A recorded fact","owner":"owner","body":"The current boundary"},"question_checked":question,"reason":"Prepare the comment target"})).await?;
    let absent = format!("absent-{}", uuid::Uuid::new_v4().simple());
    let workspaces = vec![w.ws.clone(), absent.clone()];
    let conn = w.pool.get().await?;
    let counts = || async {
        let row = conn.query_one("SELECT (SELECT COUNT(*) FROM joppa.events WHERE workspace=ANY($1)), (SELECT COUNT(*) FROM joppa.object_revisions WHERE workspace=ANY($1)), (SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=ANY($1))", &[&workspaces]).await?;
        Ok::<_, anyhow::Error>((
            row.get::<_, i64>(0),
            row.get::<_, i64>(1),
            row.get::<_, i64>(2),
        ))
    };
    for address in ["domain:security", "record:one"] {
        let original = w.get(&format!("address={address}")).await?;
        let before = counts().await?;
        let command =
            json!({"op":"note","target":address,"id":"outsider","text":"An unauthorized comment"});
        for transport in ["http", "mcp"] {
            let request_id = uuid::Uuid::new_v4().to_string();
            let (status, denied) =
                text_rule_request(&w, &w.eve, &request_id, command.clone(), transport).await?;
            let (url, body) = if transport == "http" {
                (
                    format!("{}/api/workspaces/{absent}/commands", w.root),
                    json!({"request_id":request_id,"command":command}),
                )
            } else {
                (
                    format!("{}/mcp", w.root),
                    json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":absent,"request_id":request_id,"command":command}}}),
                )
            };
            let response = w
                .http
                .post(url)
                .bearer_auth(&w.eve)
                .json(&body)
                .send()
                .await?;
            let absent_status = response.status().as_u16();
            let absent_reply: Value = response.json().await?;
            ensure!(
                status == absent_status && denied == absent_reply,
                "The refusal reveals workspace access: {status}: {denied} vs {absent_status}: {absent_reply}"
            );
            if transport == "http" {
                ensure!(
                    status == 404 && denied["error"] == "Workspace not found",
                    "{status}: {denied}"
                );
            } else {
                ensure!(
                    status == 200
                        && denied["result"]["isError"] == true
                        && denied["result"]["content"][0]["text"] == "Workspace not found",
                    "{status}: {denied}"
                );
            }
            ensure!(
                counts().await? == before,
                "The refused comment stored an event, revision or receipt"
            );
        }
        let after = w.get(&format!("address={address}")).await?;
        ensure!(
            after["document"] == original["document"]
                && after["annotations"]["comments"] == original["annotations"]["comments"],
            "The refused comment changed the object: {after}"
        );
        w.command_as(
            &w.alex,
            json!({"op":"note","target":address,"id":"member","text":"A member comment"}),
        )
        .await?;
        let current = w.get(&format!("address={address}")).await?;
        ensure!(
            current["annotations"]["comments"]["member"]["author"] == "alex"
                && current["annotations"]["comments"]["member"]["text"] == "A member comment",
            "{current}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn history_tells_a_person_from_an_agent_by_identity_type() -> Result<()> {
    // identity-type-ac1
    let w = world().await?;
    w.note("identity", 0, "Identity", "Original body", json!([]))
        .await?;
    for (token, actor, actor_type, revision) in
        [(&w.alex, "alex", "person", 1), (&w.bot, "bot", "agent", 2)]
    {
        w.command_as(token, json!({"op":"save_object","kind":"note","id":"identity","revision":revision,"parent":null,"data":{"title":"Identity","owner":"owner","body":format!("Write by {actor}"),"source":{"actor":"business value"}},"links":[],"reason":"Clarify"})).await?;
        w.command_as(
            token,
            json!({"op":"note","target":"note:identity","id":actor,"text":"Clarify the boundary"}),
        )
        .await?;
        let query = format!("address=note:identity&revision={}", revision + 1);
        let object = w.get(&query).await?;
        ensure!(
            object["document"]["actor"] == actor && object["document"]["actor_type"] == actor_type,
            "{object}"
        );
        ensure!(
            object["document"]["data"]["source"] == json!({"actor":"business value"}),
            "business data must stay unchanged: {object}"
        );
    }
    for (revision, actor, actor_type) in [(2, "alex", "person"), (3, "bot", "agent")] {
        let object = w
            .get(&format!("address=note:identity&revision={revision}"))
            .await?;
        ensure!(
            object["document"]["actor"] == actor && object["document"]["actor_type"] == actor_type,
            "{object}"
        );
        let response: Value = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.owner)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":w.ws,"address":"note:identity","revision":revision}}}))
            .send().await?.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        let mcp: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        ensure!(mcp == object, "{mcp} / {object}");
    }
    let current = w.get("address=note:identity").await?;
    ensure!(
        current["annotations"]["comments"]["alex"]["actor_type"] == "person",
        "{current}"
    );
    ensure!(
        current["annotations"]["comments"]["bot"]["actor_type"] == "agent",
        "{current}"
    );
    let snapshot = store::read(&w.pool, &w.ws, "owner", &store::Scope::full()).await?;
    for (actor, actor_type) in [("alex", "person"), ("bot", "agent")] {
        let events: Vec<_> = snapshot["history"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["actor"] == actor)
            .collect();
        ensure!(
            events.len() == 2 && events.iter().all(|event| event["actor_type"] == actor_type),
            "{events:?}"
        );
    }
    let conn = w.pool.get().await?;
    let raw: String = conn.query_one("SELECT document::text FROM joppa.object_revisions WHERE workspace=$1 AND address='note:identity' AND revision=2", &[&w.ws]).await?.get(0);
    ensure!(
        serde_json::from_str::<Value>(&raw)?["actor_type"].is_null(),
        "the read must not rewrite a stored revision"
    );
    let response = w.http.post(format!("{}/api/workspaces/{}/commands", w.root, w.ws)).bearer_auth(&w.owner)
        .json(&json!({"request_id":"change-role","command":{"op":"member","id":"alex","role":"agent"}})).send().await?;
    ensure!(
        response.status() == reqwest::StatusCode::CONFLICT,
        "{response:?}"
    );
    ensure!(
        response.json::<Value>().await?["error"]
            == "member exists; identities cannot be reassigned",
        "a role must remain immutable"
    );
    ensure!(
        w.get("address=note:identity&revision=2").await?["document"]["actor_type"] == "person",
        "the refused role change must not affect history"
    );
    ensure!(
        w.get("address=note:identity&history=true").await?["revision_count"] == 3,
        "history must return the revision count"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn any_past_revision_reads_whole_by_its_number() -> Result<()> {
    // past-revision-whole-ac1
    let w = world().await?;
    let first = json!({"title":"A","owner":"owner","body":"first text","status":"unprocessed"});
    for (revision, data) in [
        first.clone(),
        json!({"title":"B","owner":"owner","body":"second text","status":"unprocessed"}),
        json!({"title":"C","owner":"owner","body":"third text","status":"unprocessed"}),
    ]
    .into_iter()
    .enumerate()
    {
        w.command(json!({"op":"save_object","kind":"note","id":"a","revision":revision,"parent":null,"data":data,"links":[],"reason":"Save"})).await?;
    }
    let reply = w
        .http
        .get(format!("{}/api/workspaces/{}/objects", w.root, w.ws))
        .bearer_auth(&w.bot)
        .query(&[("address", "note:a"), ("revision", "1")])
        .send()
        .await?;
    let status = reply.status();
    let text = reply.text().await?;
    ensure!(status.is_success(), "{status}: {text}");
    let old: Value = serde_json::from_str(&text)?;
    ensure!(
        old["document"]["revision"] == 1 && old["document"]["data"] == first,
        "{old}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_archived_object_leaves_the_list_of_its_parent() -> Result<()> {
    // removal-is-archiving-ac1
    let w = world().await?;
    let cap = w.get("address=capability:empty").await?;
    w.command(json!({"op":"archive_object","target":"capability:empty","revision":cap["document"]["revision"],"reason":"Cut"})).await?;
    let addresses = list_addresses(&w, &w.bot, "domain:security").await?;
    ensure!(
        addresses.contains(&"capability:restrict".to_string())
            && !addresses.contains(&"capability:empty".to_string()),
        "{addresses:?}"
    );
    let read = w.get_as(&w.bot, "address=capability:empty").await?;
    ensure!(read["document"]["present"] == false, "{read}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_archived_capability_leaves_the_list_when_done_and_retired_tasks_remain() -> Result<()> {
    // removal-is-archiving-ac1
    let w = world().await?;
    let seed = json!({"op":"capability","id":"spent","title":"Finished work","domain":"security"});
    let seed: Operation = serde_json::from_value(seed)?;
    store::execute(&w.pool, &w.ws, "owner", "spent", &seed).await?;
    w.command(json!({"op":"revise","req":"spent","expected_revision":0,"capability":"spent","title":"Finished rule","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"spent-a","text":"Criterion A"}]})).await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"spent","revision":1,"ac":"spent-a","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let done = ready_task(&w, "spent-done", "spent-a").await?;
    w.command_as(&w.bot, json!({"op":"claim_task","id":done,"revision":1}))
        .await?;
    w.command_as(
        &w.bot,
        json!({"op":"complete_task","id":done,"revision":2,"evidence":"Implemented the contract"}),
    )
    .await?;
    let retired = ready_task(&w, "spent-retired", "spent-a").await?;
    w.command(json!({"op":"retire_task","id":retired,"revision":1,"reason":"The owner no longer needs this work"})).await?;
    for target in ["ac:spent-a", "requirement:spent", "capability:spent"] {
        let before = w.get(&format!("address={target}")).await?;
        w.command(json!({"op":"archive_object","target":target,"revision":before["current_revision"],"reason":"Remove the finished work"})).await?;
    }
    for task in [&done, &retired] {
        let read = w.get(&format!("address={task}")).await?;
        ensure!(read["document"]["present"] == true, "{read}");
    }
    let addresses = list_addresses(&w, &w.bot, "domain:security").await?;
    ensure!(
        addresses.contains(&"capability:restrict".to_string())
            && !addresses.contains(&"capability:spent".to_string()),
        "{addresses:?}"
    );
    let read = w.get_as(&w.bot, "address=capability:spent").await?;
    ensure!(read["document"]["present"] == false, "{read}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_requirement_with_no_live_criteria_archives_over_http() -> Result<()> {
    // removal-is-archiving-ac2: skopanev (the owner token) archives a requirement whose
    // criteria are all archived, and a requirement with no criteria.
    let w = world().await?;
    let before = w.get("address=ac:key-rights").await?;
    w.command(json!({"op":"archive_object","target":"ac:key-rights","revision":before["current_revision"],"reason":"Cut"})).await?;
    w.command(json!({"op":"save_object","kind":"requirement","id":"bare","revision":0,"parent":"capability:empty","data":{"title":"No criteria","body":"Scope","owner":"owner","consumers":["customer"]},"reason":"Define"})).await?;
    for target in ["requirement:key", "requirement:bare"] {
        let before = w.get(&format!("address={target}")).await?;
        let reply = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(&w.owner)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"archive_object","target":target,"revision":before["current_revision"],"reason":"Cut"}}))
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status == 200, "{target}: {status} {body}");
        let read = w.get(&format!("address={target}")).await?;
        ensure!(read["document"]["present"] == false, "{target}: {read}");
    }
    Ok(())
}

// removal-is-archiving-ac3
// skopanev creates and owns the workspace; alex is a human member of it.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_human_member_cannot_archive_a_capability_over_http() -> Result<()> {
    let mut w = world().await?;
    w.ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Owned by skopanev"}),
        json!({"op":"member","id":"alex","role":"human"}),
        json!({"op":"domain","id":"security","title":"Security","authority":"skopanev"}),
        json!({"op":"capability","id":"restrict","title":"Restrict access","domain":"security"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "skopanev", &format!("setup-{i}"), &op).await?;
    }
    let cap = w.get_as(&w.alex, "address=capability:restrict").await?;
    let archive = json!({"op":"archive_object","target":"capability:restrict","revision":cap["current_revision"],"reason":"Cut"});
    let (ok, reply) = send_as(&w, &w.alex, &archive).await?;
    ensure!(
        !ok && reply["error"] == "workspace owner or agent required",
        "{reply}"
    );
    let read = w.get_as(&w.alex, "address=capability:restrict").await?;
    ensure!(read["document"]["present"] == true, "{read}");
    Ok(())
}

// tree-owner-sets-types-ac2
// skopanev creates and owns the workspace; alex is a human member of it.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_human_member_cannot_change_the_tree_over_http() -> Result<()> {
    let mut w = world().await?;
    w.ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Owned by skopanev"}),
        json!({"op":"member","id":"alex","role":"human"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "skopanev", &format!("setup-{i}"), &op).await?;
    }
    let tree = async || -> Result<Value> {
        let reply = w
            .http
            .get(format!("{}/api/workspaces/{}/tree", w.root, w.ws))
            .bearer_auth(&w.skopanev)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status.is_success(), "GET /tree: {status} {body}");
        Ok(serde_json::from_str(&body)?)
    };
    let before = tree().await?;
    let n = before["current_revision"].clone();
    ensure!(n.is_u64(), "{before}");
    let mut types = before["schema"]["types"].clone();
    ensure!(types.get("job").is_none(), "{before}");
    types["job"] = json!({"title":"Jobs","role":"task","root":true,"parents":["*"]});
    let (status, reply) = text_rule_request(
        &w,
        &w.alex,
        &uuid::Uuid::new_v4().to_string(),
        json!({"op":"set_tree_schema","revision":n,"types":types,"reason":"Add jobs"}),
        "http",
    )
    .await?;
    ensure!(
        reply["error"] == "workspace owner or agent required",
        "{status} {reply}"
    );
    let after = tree().await?;
    ensure!(
        after["current_revision"] == n && after["schema"]["types"].get("job").is_none(),
        "{after}"
    );
    Ok(())
}

// tree-owner-sets-types-ac3
// bot is an agent member of the shared spec workspace.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_member_adds_risk_to_the_tree_over_http() -> Result<()> {
    let w = world().await?;
    let tree = async || -> Result<Value> {
        let reply = w
            .http
            .get(format!("{}/api/workspaces/{}/tree", w.root, w.ws))
            .bearer_auth(&w.bot)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status.is_success(), "GET /tree: {status} {body}");
        Ok(serde_json::from_str(&body)?)
    };
    let before = tree().await?;
    let n = before["current_revision"].as_u64();
    ensure!(n.is_some(), "{before}");
    let n = n.unwrap();
    let mut types = before["schema"]["types"].clone();
    ensure!(types.get("risk").is_none(), "{before}");
    types["risk"] = json!({"title":"Risk","role":"document","root":false,"parents":["capability"],"references":{"consumers":{"target":"consumer","title":"Used by","multiple":true,"required":false,"verification_subject":false}}});
    let (status, reply) = text_rule_request(
        &w,
        &w.bot,
        &uuid::Uuid::new_v4().to_string(),
        json!({"op":"set_tree_schema","revision":n,"types":types,"reason":"Add risk"}),
        "http",
    )
    .await?;
    ensure!(
        (200..300).contains(&status) && reply.get("error").is_none(),
        "{status} {reply}"
    );
    let after = tree().await?;
    ensure!(
        after["current_revision"] == n + 1 && after["schema"]["types"].get("risk").is_some(),
        "{after}"
    );
    let (actor, command) = last_event(&w).await?;
    ensure!(
        actor == "bot"
            && command["op"] == "set_tree_schema"
            && command["types"].get("risk").is_some(),
        "{actor} {command}"
    );
    Ok(())
}

// removal-is-archiving-ac5
// skopanev owns the workspace and archives a capability with one live requirement.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_cannot_archive_a_capability_with_a_live_requirement_over_http() -> Result<()> {
    let mut w = world().await?;
    w.ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Owned by skopanev"}),
        json!({"op":"consumer","id":"customer","title":"Customer","owner":"skopanev"}),
        json!({"op":"domain","id":"security","title":"Security","authority":"skopanev"}),
        json!({"op":"capability","id":"restrict","title":"Restrict access","domain":"security"}),
        json!({"op":"save_object","kind":"requirement","id":"key","revision":0,"parent":"capability:restrict","data":{"title":"Key rights","body":"Scope","owner":"skopanev","consumers":["customer"]},"reason":"Define"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "skopanev", &format!("setup-{i}"), &op).await?;
    }
    let requirement = w.get_as(&w.skopanev, "address=requirement:key").await?;
    ensure!(requirement["document"]["present"] == true, "{requirement}");
    let cap = w.get_as(&w.skopanev, "address=capability:restrict").await?;
    let archive = json!({"op":"archive_object","target":"capability:restrict","revision":cap["current_revision"],"reason":"Cut"});
    let (ok, reply) = send_as(&w, &w.skopanev, &archive).await?;
    ensure!(
        !ok && reply["error"] == "cannot archive an object with active children",
        "{reply}"
    );
    let read = w.get_as(&w.skopanev, "address=capability:restrict").await?;
    ensure!(read["document"]["present"] == true, "{read}");
    Ok(())
}

// tree-schema-safe-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn removing_a_type_with_three_objects_is_refused_over_http() -> Result<()> {
    // tree-schema-safe-ac1
    // skopanev owns the workspace; note:first, note:second and note:third exist.
    let mut w = world().await?;
    w.ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [json!({"op":"workspace","title":"Owned by skopanev"})]
        .into_iter()
        .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "skopanev", &format!("setup-{i}"), &op).await?;
    }
    let ids = ["first", "second", "third"];
    for id in ids {
        w.command_as(&w.skopanev, json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"skopanev","body":"Scope"},"links":[],"reason":"Save"})).await?;
    }
    let tree = async || -> Result<Value> {
        let reply = w
            .http
            .get(format!("{}/api/workspaces/{}/tree", w.root, w.ws))
            .bearer_auth(&w.skopanev)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status.is_success(), "GET /tree: {status} {body}");
        Ok(serde_json::from_str(&body)?)
    };
    let before = tree().await?;
    let n = before["current_revision"].clone();
    ensure!(n.is_u64(), "{before}");
    let mut types = before["schema"]["types"].clone();
    ensure!(
        types
            .as_object_mut()
            .and_then(|t| t.remove("note"))
            .is_some(),
        "{before}"
    );
    let remove = json!({"op":"set_tree_schema","revision":n,"types":types,"reason":"Remove notes"});
    for mcp in [false, true] {
        refused_without_change(
            &w,
            mcp,
            &w.skopanev,
            remove.clone(),
            "object type is absent from the tree schema",
        )
        .await?;
    }
    for id in ids {
        let note = w.get_as(&w.skopanev, &format!("address=note:{id}")).await?;
        ensure!(note["document"]["present"] == true, "{note}");
    }
    let after = tree().await?;
    ensure!(
        after["current_revision"] == n && after["schema"]["types"].get("note").is_some(),
        "{after}"
    );
    Ok(())
}

// tree-schema-safe-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_parent_rule_that_orphans_risk_objects_is_refused_over_http() -> Result<()> {
    // tree-schema-safe-ac2
    // skopanev owns the workspace; two risks sit under capability:restrict.
    let mut w = world().await?;
    w.ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [
        json!({"op":"workspace","title":"Owned by skopanev"}),
        json!({"op":"consumer","id":"customer","title":"Customer","owner":"skopanev"}),
        json!({"op":"domain","id":"security","title":"Security","authority":"skopanev"}),
        json!({"op":"capability","id":"restrict","title":"Restrict access","domain":"security"}),
    ]
    .into_iter()
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "skopanev", &format!("setup-{i}"), &op).await?;
    }
    let tree = async || -> Result<Value> {
        let reply = w
            .http
            .get(format!("{}/api/workspaces/{}/tree", w.root, w.ws))
            .bearer_auth(&w.skopanev)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status.is_success(), "GET /tree: {status} {body}");
        Ok(serde_json::from_str(&body)?)
    };
    let before = tree().await?;
    let n = before["current_revision"].as_u64();
    ensure!(n.is_some(), "{before}");
    let n = n.unwrap();
    let mut types = before["schema"]["types"].clone();
    ensure!(types.get("risk").is_none(), "{before}");
    types["risk"] = json!({"title":"Risk","role":"document","root":false,"parents":["capability"],"references":{"consumers":{"target":"consumer","title":"Used by","multiple":true,"required":false,"verification_subject":false}}});
    let (status, reply) = text_rule_request(
        &w,
        &w.skopanev,
        &uuid::Uuid::new_v4().to_string(),
        json!({"op":"set_tree_schema","revision":n,"types":types,"reason":"Add risk"}),
        "http",
    )
    .await?;
    ensure!(
        (200..300).contains(&status) && reply.get("error").is_none(),
        "{status} {reply}"
    );
    for id in ["leak", "outage"] {
        let save = json!({"op":"save_object","kind":"risk","id":id,"revision":0,"parent":"capability:restrict","data":{"title":id,"owner":"skopanev","body":"Scope"},"reason":"Define"});
        let (ok, reply) = send_as(&w, &w.skopanev, &save).await?;
        ensure!(ok, "{reply}");
    }
    types["risk"]["parents"] = json!(["domain"]);
    let narrow =
        json!({"op":"set_tree_schema","revision":n + 1,"types":types,"reason":"Move risk"});
    for mcp in [false, true] {
        refused_without_change(
            &w,
            mcp,
            &w.skopanev,
            narrow.clone(),
            "parent type is not permitted by the tree schema",
        )
        .await?;
    }
    let after = tree().await?;
    ensure!(
        after["current_revision"] == n + 1
            && after["schema"]["types"]["risk"]["parents"] == json!(["capability"]),
        "{after}"
    );
    for id in ["leak", "outage"] {
        let risk = w.get_as(&w.skopanev, &format!("address=risk:{id}")).await?;
        ensure!(
            risk["document"]["present"] == true
                && risk["document"]["parent"] == "capability:restrict",
            "{risk}"
        );
    }
    Ok(())
}

// tree-schema-safe-ac3
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_badge_change_keeps_the_object_revision_and_document_over_http() -> Result<()> {
    // tree-schema-safe-ac3
    // skopanev owns the workspace; note:kept sits at revision 2.
    use sha2::{Digest, Sha256};
    let mut w = world().await?;
    w.ws = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    for (i, op) in [json!({"op":"workspace","title":"Owned by skopanev"})]
        .into_iter()
        .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "skopanev", &format!("setup-{i}"), &op).await?;
    }
    for (revision, body) in [(0, "First"), (1, "Second")] {
        w.command_as(&w.skopanev, json!({"op":"save_object","kind":"note","id":"kept","revision":revision,"parent":null,"data":{"title":"Kept","owner":"skopanev","body":body},"links":[],"reason":"Save"})).await?;
    }
    let hash = |read: &Value| -> Result<String> {
        let document = serde_json::to_string(&read["document"])?;
        Ok(format!("{:x}", Sha256::digest(document.as_bytes())))
    };
    let http = w.get_as(&w.skopanev, "address=note:kept").await?;
    let mcp = object_as(&w, &w.skopanev, "note:kept", None).await?;
    let r = http["current_revision"].clone();
    ensure!(r == 2 && mcp["current_revision"] == r, "{http} {mcp}");
    let (http_hash, mcp_hash) = (hash(&http)?, hash(&mcp)?);
    let history = object_history(&w, "note:kept").await?;
    let tree = async || -> Result<Value> {
        let reply = w
            .http
            .get(format!("{}/api/workspaces/{}/tree", w.root, w.ws))
            .bearer_auth(&w.skopanev)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status.is_success(), "GET /tree: {status} {body}");
        Ok(serde_json::from_str(&body)?)
    };
    let before = tree().await?;
    let n = before["current_revision"].as_u64();
    ensure!(n.is_some(), "{before}");
    let n = n.unwrap();
    let mut types = before["schema"]["types"].clone();
    types["note"]["badge"] = json!("INPUT");
    let (status, reply) = text_rule_request(
        &w,
        &w.skopanev,
        &uuid::Uuid::new_v4().to_string(),
        json!({"op":"set_tree_schema","revision":n,"types":types,"reason":"Rename the note badge"}),
        "http",
    )
    .await?;
    ensure!(
        (200..300).contains(&status) && reply.get("error").is_none(),
        "{status} {reply}"
    );
    let after = tree().await?;
    ensure!(
        after["current_revision"] == n + 1 && after["schema"]["types"]["note"]["badge"] == "INPUT",
        "{after}"
    );
    let http = w.get_as(&w.skopanev, "address=note:kept").await?;
    ensure!(
        http["current_revision"] == r && hash(&http)? == http_hash,
        "{http}"
    );
    let mcp = object_as(&w, &w.skopanev, "note:kept", None).await?;
    ensure!(
        mcp["current_revision"] == r && hash(&mcp)? == mcp_hash,
        "{mcp}"
    );
    ensure!(object_history(&w, "note:kept").await? == history);
    Ok(())
}

async fn list_addresses(w: &World, token: &str, parent: &str) -> Result<Vec<String>> {
    let list = w
        .get_as(token, &format!("limit=100&parent={parent}"))
        .await?;
    Ok(list["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["address"].as_str().map(str::to_string))
        .collect())
}

// removal-is-archiving-ac4
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_archived_object_still_reads_by_address() -> Result<()> {
    let w = world().await?;
    let cap = w.get("address=capability:empty").await?;
    w.command(json!({"op":"archive_object","target":"capability:empty","revision":cap["document"]["revision"],"reason":"Cut"})).await?;
    let read = w.get("address=capability:empty").await?;
    ensure!(read["document"]["present"] == false, "{read}");
    let reply = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/objects?address=capability:empty",
            w.root, w.ws
        ))
        .bearer_auth(&w.bot)
        .send()
        .await?;
    let status = reply.status();
    let body = reply.text().await?;
    ensure!(status == 200, "{status} {body}");
    let read: Value = serde_json::from_str(&body)?;
    ensure!(read["document"]["present"] == false, "{status} {body}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn retired_task_does_not_prevent_archiving_its_parent() -> Result<()> {
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["work"] = json!({"title":"Work","role":"task","root":false,"parents":["*"]});
    types["record"] = json!({"title":"Record","role":"document","root":true,"parents":["*"]});
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check archive with configured Task and parent names"})).await?;
    w.command(json!({"op":"save_object","kind":"record","id":"parent","revision":0,"parent":null,"data":{"title":"Parent record","owner":"owner"},"reason":"Create a configured parent"})).await?;
    for (index, (parent, kind)) in [("ac:key-rights", "task"), ("record:parent", "work")]
        .into_iter()
        .enumerate()
    {
        let before = w.get(&format!("address={parent}")).await?;
        let task = format!("{kind}:archive-{index}");
        w.command(json!({"op":"save_object","kind":kind,"id":format!("archive-{index}"),"revision":0,"parent":parent,"data":{"title":"Retired work","owner":"owner","contract":"This work no longer applies","purpose":"implementation","projects":[]},"reason":"Create work under the parent"})).await?;
        let archive = json!({"op":"archive_object","target":parent,"revision":before["current_revision"],"reason":"Remove the unused parent"});
        let error: Value = serde_json::from_str(&w.refused(archive.clone()).await?)?;
        ensure!(
            error["error"] == "cannot archive an object with active children",
            "{error}"
        );
        w.command(json!({"op":"retire_task","id":task,"revision":1,"reason":"The owner no longer needs this work"})).await?;
        let retired = w.get(&format!("address={task}")).await?;
        ensure!(retired["process"]["status"] == "retired", "{retired}");
        let note = format!("note:live-child-{index}");
        w.command(json!({"op":"save_object","kind":"note","id":format!("live-child-{index}"),"revision":0,"parent":parent,"data":{"title":"Present input","owner":"owner","body":"This input still applies"},"reason":"Keep an ordinary present child"})).await?;
        let error: Value = serde_json::from_str(&w.refused(archive.clone()).await?)?;
        ensure!(
            error["error"] == "cannot archive an object with active children",
            "{error}"
        );
        w.command(json!({"op":"archive_object","target":note,"revision":1,"reason":"Archive the remaining input"})).await?;
        w.command(archive).await?;
        let after = w.get(&format!("address={parent}")).await?;
        ensure!(after["document"]["present"] == false, "{after}");
        let history = w
            .get(&format!(
                "address={parent}&revision={}",
                before["current_revision"]
            ))
            .await?;
        ensure!(
            history["document"] == before["document"],
            "The parent history changed: {history}"
        );
        let after_task = w.get(&format!("address={task}")).await?;
        ensure!(
            after_task["document"] == retired["document"]
                && after_task["process"]["status"] == "retired",
            "Archive changed the retired Task: {after_task}"
        );
        let list = w
            .get(&format!(
                "parent={}&limit=100",
                before["document"]["parent"].as_str().unwrap_or("*")
            ))
            .await?;
        ensure!(
            list["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["address"] != parent),
            "The archived parent remains in the list: {list}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn done_and_retired_tasks_do_not_block_archiving_a_criterion() -> Result<()> {
    // removal-is-archiving-ac6
    for mcp in [false, true] {
        let w = world().await?;
        w.command(json!({"op":"revise","req":"arch","expected_revision":0,"capability":"restrict","title":"Archive rule","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"arch-a","text":"Criterion A"},{"id":"arch-b","text":"Criterion B"},{"id":"arch-c","text":"Criterion C"}]})).await?;
        w.command_as(&w.bot, json!({"op":"assess","req":"arch","revision":1,"ac":"arch-a","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
        let done = ready_task(&w, "a-done", "arch-a").await?;
        w.command_as(&w.bot, json!({"op":"claim_task","id":done,"revision":1}))
            .await?;
        w.command_as(&w.bot, json!({"op":"complete_task","id":done,"revision":2,"evidence":"Implemented the contract"})).await?;
        let retired = ready_task(&w, "a-retired", "arch-a").await?;
        w.command(json!({"op":"retire_task","id":retired,"revision":1,"reason":"The owner no longer needs this work"})).await?;
        ready_task(&w, "b-open", "arch-b").await?;
        let blocked = ready_task(&w, "c-blocked", "arch-c").await?;
        w.command(json!({"op":"block_task","id":blocked,"revision":1,"reason":"Verification failed","evidence":"test x failed on abc123"})).await?;
        let blocked = w.get(&format!("address={blocked}")).await?;
        ensure!(blocked["process"]["blocked"].is_object(), "{blocked}");
        let tasks_before = [
            w.get(&format!("address={done}")).await?,
            w.get(&format!("address={retired}")).await?,
        ];
        ensure!(
            tasks_before[0]["process"]["status"] == "done"
                && tasks_before[1]["process"]["status"] == "retired",
            "{tasks_before:?}"
        );
        let archive = async |criterion: &str| -> Result<Value> {
            let before = w.get(&format!("address={criterion}")).await?;
            Ok(
                json!({"op":"archive_object","target":criterion,"revision":before["current_revision"],"reason":"Remove the criterion"}),
            )
        };

        let reply = send_over(&w, mcp, &w.owner, &archive("ac:arch-a").await?).await?;
        ensure!(reply.is_ok(), "mcp={mcp}: {reply:?}");
        let a = mcp_value(
            &w,
            "joppa_objects",
            json!({"workspace":w.ws,"address":"ac:arch-a"}),
        )
        .await?;
        ensure!(a["document"]["present"] == false, "mcp={mcp}: {a}");

        for criterion in ["ac:arch-b", "ac:arch-c"] {
            refused_without_change(
                &w,
                mcp,
                &w.owner,
                archive(criterion).await?,
                "cannot archive an object with active children",
            )
            .await?;
            let after = w.get(&format!("address={criterion}")).await?;
            ensure!(after["document"]["present"] == true, "mcp={mcp}: {after}");
        }

        for before in &tasks_before {
            let after = w
                .get(&format!("address={}", before["address"].as_str().unwrap()))
                .await?;
            ensure!(
                after["document"] == before["document"]
                    && after["process"]["status"] == before["process"]["status"],
                "mcp={mcp}: archive changed {before} -> {after}"
            );
        }
        store::check_journal(&w.pool).await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_children_api_returns_one_level_only() -> Result<()> {
    // lazy-children-ac2
    let w = world().await?;
    let list = w.get("limit=50&parent=capability:restrict").await?;
    let kinds: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["kind"].as_str())
        .collect();
    ensure!(kinds == vec!["requirement"], "{kinds:?}");
    let seed =
        json!({"op":"capability","id":"wide","title":"Twenty requirements","domain":"security"});
    let seed: Operation = serde_json::from_value(seed)?;
    store::execute(&w.pool, &w.ws, "owner", "wide", &seed).await?;
    let mut expected = std::collections::BTreeSet::new();
    for i in 0..20 {
        let req = format!("wide-{i:02}");
        let op = json!({"op":"revise","req":req,"expected_revision":0,"capability":"wide","title":format!("Rule {i}"),"body":"Scope","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":format!("{req}-a"),"text":"First check"},{"id":format!("{req}-b"),"text":"Second check"}]});
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&w.pool, &w.ws, "owner", &req, &op).await?;
        expected.insert(format!("requirement:{req}"));
    }
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let reply = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/objects?limit=50&parent=capability:wide",
            w.root, w.ws
        ))
        .bearer_auth(grant["token"].as_str().unwrap())
        .send()
        .await?;
    ensure!(reply.status() == 200, "bot list: {}", reply.text().await?);
    let list: Value = reply.json().await?;
    let items = list["items"].as_array().unwrap();
    let criteria = items.iter().filter(|i| i["kind"] == "ac").count();
    let requirements = items.iter().filter(|i| i["kind"] == "requirement").count();
    let addresses: std::collections::BTreeSet<String> = items
        .iter()
        .filter_map(|i| i["address"].as_str().map(str::to_string))
        .collect();
    ensure!(
        items.len() == 20 && requirements == 20 && criteria == 0 && addresses == expected,
        "{list}"
    );
    Ok(())
}

// [ac:id-survives-move-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn rename_and_move_keep_the_address_and_the_item_id() -> Result<()> {
    let w = world().await?;
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", Some("write")).await?;
    let bot = grant["token"].as_str().unwrap();
    let read = || async {
        let reply = w
            .http
            .get(format!(
                "{}/api/workspaces/{}/objects?address=requirement:key",
                w.root, w.ws
            ))
            .bearer_auth(bot)
            .send()
            .await?;
        ensure!(reply.status() == 200, "read: {}", reply.text().await?);
        Ok::<Value, anyhow::Error>(reply.json().await?)
    };
    let before = read().await?;
    ensure!(
        before["item_id"].as_str().is_some_and(|id| !id.is_empty()),
        "{before}"
    );
    ensure!(
        before["document"]["parent"] != "capability:empty",
        "{before}"
    );
    let mut data = before["document"]["data"].clone();
    data["title"] = json!("Key rights, renamed");
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(bot)
        .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"save_object","kind":"requirement","id":"key","revision":before["document"]["revision"],"parent":"capability:empty","data":data,"links":[],"reason":"Rename and move"}}))
        .send()
        .await?;
    ensure!(reply.status() == 200, "write: {}", reply.text().await?);
    let after = read().await?;
    ensure!(
        after["address"] == before["address"]
            && after["item_id"] == before["item_id"]
            && after["document"]["parent"] == "capability:empty"
            && after["document"]["data"]["title"] == "Key rights, renamed",
        "{before} -> {after}"
    );
    Ok(())
}

// [ac:id-survives-move-ac2]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_move_under_a_domain_is_refused_and_changes_nothing() -> Result<()> {
    let w = world().await?;
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", Some("write")).await?;
    let bot = grant["token"].as_str().unwrap();
    let read = || async {
        let reply = w
            .http
            .get(format!(
                "{}/api/workspaces/{}/objects?address=requirement:key",
                w.root, w.ws
            ))
            .bearer_auth(bot)
            .send()
            .await?;
        ensure!(reply.status() == 200, "read: {}", reply.text().await?);
        Ok::<Value, anyhow::Error>(reply.json().await?)
    };
    let move_to = |current: &Value, parent: &str| json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"save_object","kind":"requirement","id":"key","revision":current["document"]["revision"],"parent":parent,"data":current["document"]["data"],"links":[],"reason":"Move"}});
    let commands = format!("{}/api/workspaces/{}/commands", w.root, w.ws);
    let start = read().await?;
    let reply = w
        .http
        .post(&commands)
        .bearer_auth(bot)
        .json(&move_to(&start, "capability:empty"))
        .send()
        .await?;
    let status = reply.status();
    ensure!(
        status == 200,
        "control move: {status} {}",
        reply.text().await?
    );
    let before = read().await?;
    ensure!(
        before["item_id"].as_str().is_some_and(|id| !id.is_empty())
            && before["document"]["parent"] == "capability:empty",
        "{before}"
    );
    let reply = w
        .http
        .post(&commands)
        .bearer_auth(bot)
        .json(&move_to(&before, "domain:security"))
        .send()
        .await?;
    let status = reply.status();
    let body: Value = reply.json().await?;
    ensure!(
        status == 409 && body["error"] == "parent type is not permitted by the tree schema",
        "refused move: {status} {body}"
    );
    let after = read().await?;
    ensure!(
        after["address"] == before["address"]
            && after["item_id"] == before["item_id"]
            && after["document"]["parent"] == before["document"]["parent"]
            && after["document"]["revision"] == before["document"]["revision"],
        "{before} -> {after}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_stored_revision_on_an_ordering_link_asks_for_no_review() -> Result<()> {
    // blocking-pins-revision-ac1, narrowed by dependencies-before-priority: only a Task
    // dependency pins its target. A note keeps the stored revision, and it gates nothing.
    let w = world().await?;
    w.note("target", 0, "Target", "v1", json!([])).await?;
    w.note(
        "source",
        0,
        "Source",
        "s",
        json!([{"target":"note:target","relation":"depends_on","revision":1,"blocking":true}]),
    )
    .await?;
    w.note("target", 1, "Target", "v2", json!([])).await?;
    let source = w.get("address=note:source").await?;
    ensure!(
        source["document"]["links"][0]["revision"] == 1
            && source["document"]["links"][0]["blocking"] == true
            && source["review_required"] == false,
        "{source}"
    );
    Ok(())
}

async fn journal_refuses(sql: &str) -> Result<()> {
    // As an application role that has UPDATE and DELETE rights, so only the WORM
    // trigger can refuse; a superuser could drop the trigger and prove nothing.
    let w = world().await?;
    let mut conn = w.pool.get().await?;
    conn.batch_execute(
        "DO $$ BEGIN
            IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'joppa_test_app') THEN
                CREATE ROLE joppa_test_app NOLOGIN NOSUPERUSER;
            END IF;
         END $$;
         GRANT USAGE ON SCHEMA joppa TO joppa_test_app;
         GRANT SELECT, UPDATE, DELETE ON joppa.events TO joppa_test_app;",
    )
    .await?;
    let tx = conn.transaction().await?;
    tx.batch_execute("SET LOCAL ROLE joppa_test_app").await?;
    let error = tx
        .execute(sql, &[&w.ws])
        .await
        .expect_err("the journal must refuse");
    let db = error.as_db_error().expect("a database error");
    ensure!(
        db.code().code() == "P0001"
            && db.message() == "WORM: append a correction; history cannot be changed",
        "{error}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_journal_refuses_update() -> Result<()> {
    // record-append-only-ac1
    journal_refuses("UPDATE joppa.events SET actor=actor WHERE workspace=$1").await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_journal_refuses_delete() -> Result<()> {
    // record-append-only-ac2
    journal_refuses("DELETE FROM joppa.events WHERE workspace=$1").await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn search_finds_words_from_bodies_and_titles() -> Result<()> {
    // full-text-search-ac1, full-text-search-ac2
    let w = world().await?;
    w.note(
        "body-hit",
        0,
        "Unrelated title",
        "Users sign in with a passkey",
        json!([]),
    )
    .await?;
    w.note("title-hit", 0, "Passkey recovery", "Plain body", json!([]))
        .await?;
    let found = w.search("passkey").await?;
    ensure!(
        found.contains(&"note:body-hit".into()) && found.contains(&"note:title-hit".into()),
        "{found:?}"
    );
    Ok(())
}

// [ac:full-text-search-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot_finds_a_requirement_by_a_word_from_its_body() -> Result<()> {
    let w = world().await?;
    w.command(json!({"op":"revise","req":"sign-in","expected_revision":0,"capability":"restrict","title":"Sign in","body":"Users sign in with a passkey","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"sign-in-works","text":"Only members sign in"}]}))
        .await?;
    let result = mcp_call(
        &w,
        &w.bot,
        "joppa_search",
        json!({"workspace":w.ws,"query":"passkey"}),
    )
    .await?;
    ensure!(result["isError"] != true, "{result}");
    let found: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap_or("null"))?;
    let addresses: Vec<String> = found["results"]
        .as_array()
        .or(found["items"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|r| Some(format!("{}:{}", r["kind"].as_str()?, r["id"].as_str()?)))
        .collect();
    ensure!(
        addresses.contains(&"requirement:sign-in".into()),
        "{addresses:?} in {found}"
    );
    Ok(())
}

// [ac:full-text-search-ac2]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot_finds_a_requirement_by_a_word_from_its_title() -> Result<()> {
    let w = world().await?;
    w.command(json!({"op":"revise","req":"recovery","expected_revision":0,"capability":"restrict","title":"Recover a passkey","body":"Users regain access","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"recovery-works","text":"Only members recover access"}]}))
        .await?;
    let result = mcp_call(
        &w,
        &w.bot,
        "joppa_search",
        json!({"workspace":w.ws,"query":"passkey"}),
    )
    .await?;
    ensure!(result["isError"] != true, "{result}");
    let found: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap_or("null"))?;
    let addresses: Vec<String> = found["results"]
        .as_array()
        .or(found["items"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|r| Some(format!("{}:{}", r["kind"].as_str()?, r["id"].as_str()?)))
        .collect();
    ensure!(
        addresses.contains(&"requirement:recovery".into()),
        "{addresses:?} in {found}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn search_omits_archived_objects() -> Result<()> {
    // search-respects-access-ac1
    let w = world().await?;
    w.note("old", 0, "Legacy flow", "legacy", json!([])).await?;
    let (live, reply) = bot_search(&w, "legacy").await?;
    ensure!(live.contains(&"note:old".into()), "{reply}");
    let read = w.get("address=note:old").await?;
    w.command(json!({"op":"archive_object","target":"note:old","revision":read["document"]["revision"],"reason":"Cut"})).await?;
    let (found, reply) = bot_search(&w, "legacy").await?;
    ensure!(!found.contains(&"note:old".into()), "{reply}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn search_never_returns_another_workspaces_objects() -> Result<()> {
    // search-respects-access-ac2
    let a = world().await?;
    let b = world().await?;
    b.note(
        "secret",
        0,
        "Passkey plan",
        "passkey in workspace B only",
        json!([]),
    )
    .await?;
    let (control, reply) = bot_search(&b, "passkey").await?;
    ensure!(control.contains(&"note:secret".into()), "{reply}");
    let (found, reply) = bot_search(&a, "passkey").await?;
    ensure!(found.is_empty(), "{reply}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn another_workspace_answers_exactly_like_an_absent_one() -> Result<()> {
    // workspaces-isolated-ac1, invited-people-only-ac1: a stranger must not learn that B exists.
    let w = world().await?;
    let other = format!("http-spec-b-{}", uuid::Uuid::new_v4().simple());
    let create: Operation = serde_json::from_value(json!({"op":"workspace","title":"B"}))?;
    store::execute(&w.pool, &other, "someone-else", "create-b", &create).await?;
    let read = |ws: String| {
        let w = &w;
        async move {
            let reply = w
                .http
                .get(format!(
                    "{}/api/workspaces/{ws}/objects?limit=5&kind=domain",
                    w.root
                ))
                .bearer_auth(&w.owner)
                .send()
                .await?;
            Ok::<_, anyhow::Error>((reply.status().as_u16(), reply.text().await?))
        }
    };
    let existing = read(other).await?;
    let absent = read(format!("absent-{}", uuid::Uuid::new_v4().simple())).await?;
    ensure!(
        existing.0 == 404 && existing == absent,
        "{existing:?} vs {absent:?}"
    );
    ensure!(existing.1.contains("Workspace not found"), "{existing:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_account_without_an_invite_cannot_read_an_object_over_http() -> Result<()> {
    // [ac:invited-people-only-ac1] eve reads an object and gets the absent-workspace reply.
    let w = world().await?;
    let id = uuid::Uuid::new_v4().simple();
    let title = format!("Private title {id}");
    let body = format!("Private body {id}");
    w.note("eve-target", 0, &title, &body, json!([])).await?;
    let read = |ws: String, token: String| {
        let w = &w;
        async move {
            let reply = w
                .http
                .get(format!(
                    "{}/api/workspaces/{ws}/objects?address=note:eve-target",
                    w.root
                ))
                .bearer_auth(token)
                .send()
                .await?;
            Ok::<_, anyhow::Error>((reply.status().as_u16(), reply.text().await?))
        }
    };
    let owner = read(w.ws.clone(), w.owner.clone()).await?;
    let saved: Value = serde_json::from_str(&owner.1)?;
    ensure!(
        owner.0 == 200 && saved["document"]["data"]["title"] == title.as_str(),
        "{owner:?}"
    );
    let member_only = read(w.ws.clone(), w.eve.clone()).await?;
    let absent = read(
        format!("absent-{}", uuid::Uuid::new_v4().simple()),
        w.eve.clone(),
    )
    .await?;
    for reply in [&member_only, &absent] {
        ensure!(
            reply.0 == 404 && reply.1 == r#"{"error":"Workspace not found"}"#,
            "{member_only:?} vs {absent:?}"
        );
        ensure!(
            !reply.1.contains(&title) && !reply.1.contains(&body),
            "{member_only:?} vs {absent:?}"
        );
    }
    ensure!(member_only == absent, "{member_only:?} vs {absent:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_without_membership_cannot_read_a_workspace_over_mcp() -> Result<()> {
    // agent-needs-grant-ac1
    let w = world().await?;
    let other = format!("http-spec-b-{}", uuid::Uuid::new_v4().simple());
    let secret = format!("private-body-{}", uuid::Uuid::new_v4().simple());
    for (index, command) in [
        json!({"op":"workspace","title":"Private workspace B"}),
        json!({"op":"save_object","kind":"note","id":"private","revision":0,"parent":null,"data":{"title":"Private note","owner":"someone-else","body":secret},"reason":"Test private workspace data"}),
    ]
    .into_iter()
    .enumerate()
    {
        let command: Operation = serde_json::from_value(command)?;
        store::execute(
            &w.pool,
            &other,
            "someone-else",
            &format!("private-{index}"),
            &command,
        )
        .await?;
    }
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let read = |workspace: String| {
        let w = &w;
        async move {
            let reply = w
                .http
                .post(format!("{}/mcp", w.root))
                .bearer_auth(bot)
                .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_read","arguments":{"workspace":workspace}}}))
                .send()
                .await?;
            ensure!(reply.status().is_success(), "{}", reply.status());
            let reply: Value = reply.json().await?;
            ensure!(reply.get("error").is_none(), "{reply}");
            Ok::<_, anyhow::Error>(reply["result"].clone())
        }
    };
    let allowed = read(w.ws.clone()).await?;
    ensure!(allowed["isError"] != true, "{allowed}");
    let allowed: Value = serde_json::from_str(
        allowed["content"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("The allowed read has no document"))?,
    )?;
    ensure!(allowed["workspace"] == w.ws, "{allowed}");
    let existing = read(other).await?;
    let absent = read(format!("absent-{}", uuid::Uuid::new_v4().simple())).await?;
    ensure!(existing == absent, "{existing} vs {absent}");
    for denied in [existing, absent] {
        ensure!(denied["isError"] == true, "{denied}");
        ensure!(
            denied["content"] == json!([{"type":"text","text":"Workspace not found"}]),
            "{denied}"
        );
        ensure!(
            !denied.to_string().contains(&secret),
            "The refused read exposed the private note"
        );
        ensure!(
            denied.get("structuredContent").is_none(),
            "The refused read included document data: {denied}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_required_source_refuses_an_agent_write_without_a_source() -> Result<()> {
    // signal-records-source-ac2
    let w = world().await?;
    w.note("archived", 0, "Archived input", "No source", json!([]))
        .await?;
    w.command(json!({"op":"archive_object","target":"note:archived","revision":1,"reason":"Archive before requiring a source"})).await?;
    w.note("original", 0, "Original input", "Keep this text", json!([]))
        .await?;
    let original = w.get("address=note:original").await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["note"]["required_fields"] = json!({"source":"a signal needs a source"});
    types["feedback"] = json!({"title":"Feedback","role":"document","root":true,"parents":["*"],"required_fields":{"source":"a signal needs a source"}});
    types["record"] = json!({"title":"Record","role":"document","root":true,"parents":["*"]});
    let schema_command = json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Require the configured source field"});
    let error = w.refused(schema_command.clone()).await?;
    ensure!(error.contains("a signal needs a source"), "{error}");
    let after = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    ensure!(after == schema, "A refused schema changed: {after}");
    ensure!(
        w.get("address=note:original").await?["document"] == original["document"],
        "A refused schema changed the original input"
    );
    let source = json!({"channel":"web","author":"alex","at":"2026-10-09T10:00:00Z"});
    let mut data = original["document"]["data"].clone();
    data["source"] = source.clone();
    w.command(json!({"op":"save_object","kind":"note","id":"original","revision":1,"parent":null,"data":data,"reason":"Supply the source before enabling the rule"})).await?;
    w.command(schema_command).await?;
    let archived = w.pool.get().await?.query_one("SELECT present, document::text FROM joppa.objects WHERE workspace=$1 AND address='note:archived'", &[&w.ws]).await?;
    let present: bool = archived.get(0);
    let document: Value = serde_json::from_str(archived.get(1))?;
    ensure!(
        !present && document["data"].get("source").is_none(),
        "An archived input must not block or receive the required field: {document}"
    );
    let history = w.get("address=note:original&revision=1").await?;
    ensure!(
        history["document"] == original["document"],
        "The source rule changed a historical revision: {history}"
    );
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = grant["token"].as_str().unwrap();
    for kind in ["note", "feedback"] {
        for transport in ["http", "mcp"] {
            let id = format!("{kind}-{transport}");
            let address = format!("{kind}:{id}");
            let request_id = uuid::Uuid::new_v4().to_string();
            let mut command = json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":null,"data":{"title":"New input","owner":"owner","body":"Input text"},"reason":"Check the configured source rule"});
            let conn = w.pool.get().await?;
            let before: i64 = conn
                .query_one(
                    "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
                    &[&w.ws],
                )
                .await?
                .get(0);
            let request = |command: Value| {
                let w = &w;
                let request_id = &request_id;
                async move {
                    let (url, body) = if transport == "http" {
                        (
                            format!("{}/api/workspaces/{}/commands", w.root, w.ws),
                            json!({"request_id":request_id,"command":command}),
                        )
                    } else {
                        (
                            format!("{}/mcp", w.root),
                            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":request_id,"command":command}}}),
                        )
                    };
                    let response = w.http.post(url).bearer_auth(bot).json(&body).send().await?;
                    Ok::<_, anyhow::Error>((
                        response.status().as_u16(),
                        response.json::<Value>().await?,
                    ))
                }
            };
            let (status, denied) = request(command.clone()).await?;
            if transport == "http" {
                ensure!(
                    status == 422 && denied["error"] == "a signal needs a source",
                    "{status}: {denied}"
                );
            } else {
                ensure!(
                    status == 200
                        && denied["result"]["isError"] == true
                        && denied["result"]["content"][0]["text"] == "a signal needs a source",
                    "{status}: {denied}"
                );
            }
            let after: i64 = conn
                .query_one(
                    "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
                    &[&w.ws],
                )
                .await?
                .get(0);
            ensure!(after == before, "A refused write appended an event");
            let revisions: i64 = conn
                .query_one(
                    "SELECT COUNT(*) FROM joppa.object_revisions WHERE workspace=$1 AND address=$2",
                    &[&w.ws, &address],
                )
                .await?
                .get(0);
            ensure!(revisions == 0, "A refused write created a revision");
            let receipts: i64 = conn.query_one("SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2", &[&w.ws, &request_id]).await?.get(0);
            ensure!(receipts == 0, "A refused write consumed its request ID");
            drop(conn);
            command["data"]["source"] = source.clone();
            let (status, saved) = request(command).await?;
            ensure!(status == 200, "{status}: {saved}");
            if transport == "mcp" {
                ensure!(saved["result"]["isError"] != true, "{saved}");
            }
            let read = w.get(&format!("address={address}")).await?;
            ensure!(
                read["document"]["data"]["source"] == source && read["current_revision"] == 1,
                "{read}"
            );
        }
    }
    let error = w.refused(json!({"op":"save_object","kind":"note","id":"original","revision":2,"parent":null,"data":original["document"]["data"],"reason":"Try to remove the required source"})).await?;
    ensure!(error.contains("a signal needs a source"), "{error}");
    let original_after = w.get("address=note:original").await?;
    ensure!(
        original_after["current_revision"] == 2
            && original_after["document"]["data"]["source"] == source,
        "{original_after}"
    );
    w.command_as(bot, json!({"op":"save_object","kind":"record","id":"plain","revision":0,"parent":null,"data":{"title":"Ordinary record","owner":"owner"},"reason":"The schema does not require source for this type"})).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_description_without_given_is_refused() -> Result<()> {
    configured_description_without_marker_is_refused("Given:").await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_description_without_when_is_refused() -> Result<()> {
    configured_description_without_marker_is_refused("When:").await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_description_without_then_is_refused() -> Result<()> {
    configured_description_without_marker_is_refused("Then:").await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_description_without_evidence_is_refused() -> Result<()> {
    configured_description_without_marker_is_refused("Evidence:").await
}

async fn text_rule_request(
    w: &World,
    token: &str,
    request_id: &str,
    command: Value,
    transport: &str,
) -> Result<(u16, Value)> {
    let (url, body) = if transport == "http" {
        (
            format!("{}/api/workspaces/{}/commands", w.root, w.ws),
            json!({"request_id":request_id,"command":command}),
        )
    } else {
        (
            format!("{}/mcp", w.root),
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":request_id,"command":command}}}),
        )
    };
    let response = w
        .http
        .post(url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;
    Ok((response.status().as_u16(), response.json().await?))
}

async fn text_rule_refusal(
    w: &World,
    token: &str,
    request_id: &str,
    command: Value,
    error: &str,
) -> Result<()> {
    let conn = w.pool.get().await?;
    let sql = "SELECT (SELECT COUNT(*) FROM joppa.events WHERE workspace=$1), (SELECT COUNT(*) FROM joppa.object_revisions WHERE workspace=$1), (SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2)";
    let counts = || async {
        let row = conn.query_one(sql, &[&w.ws, &request_id]).await?;
        Ok::<_, anyhow::Error>((
            row.get::<_, i64>(0),
            row.get::<_, i64>(1),
            row.get::<_, i64>(2),
        ))
    };
    let before = counts().await?;
    ensure!(before.2 == 0, "The request ID already has a receipt");
    for transport in ["http", "mcp"] {
        let (status, denied) =
            text_rule_request(w, token, request_id, command.clone(), transport).await?;
        if transport == "http" {
            ensure!(
                status == 422 && denied["error"] == error,
                "{status}: {denied}"
            );
        } else {
            ensure!(
                status == 200
                    && denied["result"]["isError"] == true
                    && denied["result"]["content"][0]["text"] == error,
                "{status}: {denied}"
            );
        }
        ensure!(
            counts().await? == before,
            "A refused write stored an event, revision or receipt"
        );
    }
    Ok(())
}

async fn configured_description_without_marker_is_refused(missing: &str) -> Result<()> {
    let w = world().await?;
    let markers = ["Given:", "When:", "Then:", "Evidence:"];
    let valid = "Given: an input.\nWhen: the input changes.\nThen: the contract changes.\nEvidence: the saved contract.";
    let invalid = markers
        .iter()
        .filter(|marker| **marker != missing)
        .map(|marker| format!("{marker} a check."))
        .collect::<Vec<_>>()
        .join("\n");
    let error = "an ac description needs Given:, When:, Then: and Evidence:";
    let criterion = w.get("address=ac:key-rights").await?;
    let mut data = criterion["document"]["data"].clone();
    data["description"] = json!(valid);
    w.command(json!({"op":"save_object","kind":"ac","id":"key-rights","revision":criterion["current_revision"],"parent":criterion["document"]["parent"],"data":data,"reason":"Prepare existing criterion text"})).await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["observation"] =
        json!({"title":"Observation","role":"document","root":true,"parents":["*"]});
    types["record"] = json!({"title":"Record","role":"document","root":true,"parents":["*"]});
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Add ordinary document types"})).await?;
    for id in ["original", "archived"] {
        w.command(json!({"op":"save_object","kind":"observation","id":id,"revision":0,"parent":null,"data":{"title":"An observation","owner":"owner","description":"Original text"},"reason":"Keep original text"})).await?;
    }
    let original = w.get("address=observation:original").await?;
    w.command(json!({"op":"archive_object","target":"observation:archived","revision":1,"reason":"Archive before the new rule"})).await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    let question = "Which one check proves the rule?";
    let rule = json!({"contains":markers,"error":error});
    for kind in ["ac", "observation"] {
        types[kind]["question"] = json!(question);
        types[kind]["required_fields"] = json!({"description":rule});
    }
    if missing == "Given:" {
        types["domain"]["required_fields"] = json!({"title":{"contains":["Sec","urity"],"error":"A domain title needs both markers"}});
    }
    let command = json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Require literal text markers"});
    let schema_request = uuid::Uuid::new_v4().to_string();
    text_rule_refusal(&w, &w.owner, &schema_request, command.clone(), error).await?;
    ensure!(
        store::tree_schema(&w.pool, &w.ws, "owner", None).await? == schema,
        "A refused schema changed"
    );
    let mut data = original["document"]["data"].clone();
    data["description"] = json!(valid);
    w.command(json!({"op":"save_object","kind":"observation","id":"original","revision":1,"parent":null,"data":data,"reason":"Prepare current text before the rule"})).await?;
    let (status, saved) = text_rule_request(&w, &w.owner, &schema_request, command, "http").await?;
    ensure!(status == 200, "{status}: {saved}");
    ensure!(
        w.get("address=observation:original&revision=1").await?["document"] == original["document"],
        "The rule changed historical text"
    );
    let archived = w.get("address=observation:archived").await?;
    ensure!(
        archived["document"]["present"] == false
            && archived["document"]["data"]["description"] == "Original text",
        "The rule changed archived data: {archived}"
    );
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = grant["token"].as_str().unwrap();
    for kind in ["ac", "observation"] {
        let parent = if kind == "ac" {
            json!("requirement:key")
        } else {
            Value::Null
        };
        let mut command = json!({"op":"save_object","kind":kind,"id":"new","revision":0,"parent":parent,"data":{"title":"The input changes the contract","owner":"owner","description":invalid},"reason":"Check the configured text rule"});
        let request_id = uuid::Uuid::new_v4().to_string();
        if missing == "Given:" {
            let question_error = format!(
                "Check that the title and description answer: {question}. Rewrite them if they do not. Then resend with question_checked"
            );
            text_rule_refusal(&w, bot, &request_id, command.clone(), &question_error).await?;
            command["question_checked"] = json!("A different question");
            text_rule_refusal(&w, bot, &request_id, command.clone(), &question_error).await?;
        }
        command["question_checked"] = json!(question);
        text_rule_refusal(&w, bot, &request_id, command.clone(), error).await?;
        command["data"]["description"] = json!(valid);
        let transport = if kind == "ac" { "http" } else { "mcp" };
        let (status, saved) =
            text_rule_request(&w, bot, &request_id, command.clone(), transport).await?;
        ensure!(status == 200, "{status}: {saved}");
        if transport == "mcp" {
            ensure!(saved["result"]["isError"] != true, "{saved}");
        }
        let address = format!("{kind}:new");
        let saved = w.get(&format!("address={address}")).await?;
        ensure!(
            saved["current_revision"] == 1
                && command["data"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .all(|(field, value)| saved["document"]["data"].get(field) == Some(value)),
            "{saved}"
        );
        command["revision"] = json!(1);
        command["data"]["description"] = json!(invalid);
        text_rule_refusal(&w, bot, &uuid::Uuid::new_v4().to_string(), command, error).await?;
        ensure!(
            w.get(&format!("address={address}")).await?["document"] == saved["document"],
            "The refused update changed the document"
        );
    }
    w.command_as(bot, json!({"op":"save_object","kind":"record","id":"plain","revision":0,"parent":null,"data":{"title":"An ordinary record","owner":"owner","description":"No markers"},"reason":"Keep a type without a rule unchanged"})).await?;
    if missing == "Given:" {
        text_rule_refusal(
            &w,
            bot,
            &uuid::Uuid::new_v4().to_string(),
            json!({"op":"domain","id":"legacy","title":"No markers","owner":"owner"}),
            "A domain title needs both markers",
        )
        .await?;
        w.command_as(
            bot,
            json!({"op":"domain","id":"legacy","title":"Security","owner":"owner"}),
        )
        .await?;
        ensure!(w.get("address=domain:legacy").await?["document"]["data"]["title"] == "Security");
        let legacy_text = "Given: input. When: change. Then: saved. Evidence: API.";
        text_rule_refusal(&w, bot, &uuid::Uuid::new_v4().to_string(), json!({"op":"revise","req":"key","expected_revision":w.get("address=requirement:key").await?["process"]["revision"],"capability":"restrict","title":"Key rights","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Try a legacy criterion edit","acs":[{"id":"key-rights","text":legacy_text},{"id":"legacy-question","text":legacy_text}]}), "This type has a question. Use save_object with question_checked").await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn legacy_document_commands_obey_configured_required_fields() -> Result<()> {
    let w = world().await?;
    let domain = w.get("address=domain:security").await?;
    let mut data = domain["document"]["data"].clone();
    data["origin"] = json!("The agreed security scope");
    w.command(json!({"op":"save_object","kind":"domain","id":"security","revision":domain["current_revision"],"parent":null,"data":data,"reason":"Supply the origin before enabling the rule"})).await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["domain"]["required_fields"] = json!({"origin":"origin is required"});
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Apply the rule to legacy document writes"})).await?;
    let conn = w.pool.get().await?;
    let before: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    let error = w.refused(json!({"op":"domain","id":"no-origin","title":"A domain without an origin","owner":"owner"})).await?;
    let error: Value = serde_json::from_str(&error)?;
    ensure!(error["error"] == "origin is required", "{error}");
    let after: i64 = conn
        .query_one(
            "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(
        before == after,
        "The refused legacy command appended an event"
    );
    let revisions: i64 = conn.query_one("SELECT COUNT(*) FROM joppa.object_revisions WHERE workspace=$1 AND address='domain:no-origin'", &[&w.ws]).await?.get(0);
    ensure!(
        revisions == 0,
        "The refused legacy command created a revision"
    );
    Ok(())
}

impl World {
    /// The error text of a command that must be refused.
    async fn refused(&self, command: Value) -> Result<String> {
        let reply = self
            .http
            .post(format!("{}/api/workspaces/{}/commands", self.root, self.ws))
            .bearer_auth(&self.owner)
            .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
            .send()
            .await?;
        ensure!(!reply.status().is_success(), "{command} must be refused");
        Ok(reply.text().await?)
    }
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_edit_adds_a_revision_and_keeps_the_original() -> Result<()> {
    // edits-keep-original-ac1
    let w = world().await?;
    w.note("a", 0, "A", "original text", json!([])).await?;
    w.note("a", 1, "A", "edited text", json!([])).await?;
    let current = w.get("address=note:a").await?;
    let first = w.get("address=note:a&revision=1").await?;
    ensure!(
        current["document"]["revision"] == 2
            && current["document"]["data"]["body"] == "edited text",
        "{current}"
    );
    ensure!(
        first["document"]["data"]["body"] == "original text",
        "{first}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_edit_on_a_stale_revision_is_refused() -> Result<()> {
    // edits-keep-original-ac2
    let w = world().await?;
    w.note("a", 0, "A", "original text", json!([])).await?;
    w.note("a", 1, "A", "edited text", json!([])).await?;
    let error = w
        .refused(json!({"op":"save_object","kind":"note","id":"a","revision":1,"parent":null,"data":{"title":"A","owner":"owner","body":"late text"},"links":[],"reason":"Save"}))
        .await?;
    ensure!(
        error.contains("stale object revision; read the object again"),
        "{error}"
    );
    let current = w.get("address=note:a").await?;
    ensure!(
        current["document"]["data"]["body"] == "edited text",
        "{current}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_link_to_another_workspace_is_refused() -> Result<()> {
    // workspaces-isolated-ac2
    let a = world().await?;
    let b = world().await?;
    b.note("only-b", 0, "B", "lives in B", json!([])).await?;
    a.note("a", 0, "A", "lives in A", json!([])).await?;
    let error = a
        .refused(json!({"op":"save_object","kind":"note","id":"a","revision":1,"parent":null,"data":{"title":"A","owner":"owner","body":"lives in A"},"links":[{"target":"note:only-b","relation":"related"}],"reason":"Link"}))
        .await?;
    ensure!(
        error.contains("unknown linked object in this workspace"),
        "{error}"
    );

    // links-in-workspace-ac1: the bot holds a write grant for workspace A only.
    let grant = store::create_agent_token(&a.pool, &a.ws, "owner", "bot", Some("write")).await?;
    let bot = grant["token"].as_str().unwrap();
    let stored = store::token_actor(&a.pool, bot).await?.unwrap();
    ensure!(
        stored.agent == "bot" && stored.workspace == a.ws && stored.access == store::Access::Write,
        "{stored:?}"
    );
    a.note("also-a", 0, "Also A", "lives in A", json!([]))
        .await?;
    let link = |revision: u32, target: &'static str| {
        let a = &a;
        async move {
            let reply = a
                .http
                .post(format!("{}/api/workspaces/{}/commands", a.root, a.ws))
                .bearer_auth(bot)
                .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"save_object","kind":"note","id":"a","revision":revision,"parent":null,"data":{"title":"A","owner":"owner","body":"lives in A"},"links":[{"target":target,"relation":"related"}],"reason":"Link"}}))
                .send()
                .await?;
            let status = reply.status();
            let text = reply.text().await?;
            Ok::<_, anyhow::Error>((status, serde_json::from_str(&text).unwrap_or(json!(text))))
        }
    };

    // The control: the same credential links two objects inside A.
    let (status, reply) = link(1, "note:also-a").await?;
    ensure!(status.is_success(), "control link: {status} {reply}");
    let before = saved_rows(&a).await?;
    ensure!(
        before["object_links"].as_array().is_some_and(|links| links
            .iter()
            .any(|l| l["source"] == "note:a"
                && l["target"] == "note:also-a"
                && l["relation"] == "related")),
        "{}",
        before["object_links"]
    );
    ensure!(
        before["objects"]
            .as_array()
            .is_some_and(|objects| objects.iter().all(|o| o["address"] != "note:only-b")),
        "note:only-b must exist only in workspace B: {}",
        before["objects"]
    );

    // The link to the address that exists only in B is refused and changes nothing in A.
    let (status, reply) = link(2, "note:only-b").await?;
    ensure!(
        !status.is_success(),
        "cross-workspace link: {status} {reply}"
    );
    ensure!(
        reply["error"] == "unknown linked object in this workspace",
        "{status} {reply}"
    );
    let after = saved_rows(&a).await?;
    for table in ["objects", "object_revisions", "object_links"] {
        ensure!(
            after[table] == before[table],
            "{table} changed after the refusal: {} -> {}",
            before[table],
            after[table]
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_inbox_lists_undecided_inputs_before_decided_inputs() -> Result<()> {
    // undecided-first-ac1
    let w = world().await?;
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let token = grant["token"].as_str().unwrap();
    for id in ["a-decided", "b-decided", "z-undecided"] {
        w.note(id, 0, id, "Input", json!([])).await?;
        if id != "z-undecided" {
            w.command(json!({"op":"decide_note","target":format!("note:{id}"),"revision":1,"outcome":"changed_spec","reason":"Input reviewed","links":["domain:security"]})).await?;
        }
    }
    let expected = ["note:z-undecided", "note:a-decided", "note:b-decided"];
    let http = w.get("kind=note&parent=*&limit=100").await?;
    ensure!(http["items"][0]["status"] == "unprocessed", "{http}");
    let mut cursor = None;
    for (i, address) in expected.into_iter().enumerate() {
        let mut arguments = json!({"workspace":w.ws,"kind":"note","parent":"*","limit":1});
        if let Some(cursor) = cursor {
            arguments["cursor"] = cursor;
        }
        let response: Value = w.http.post(format!("{}/mcp", w.root)).bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":arguments}}))
            .send().await?.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        let page: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        ensure!(
            page["items"].as_array().unwrap().len() == 1 && page["items"][0]["address"] == address,
            "{page}"
        );
        ensure!(page["items"][0] == http["items"][i], "{page} / {http}");
        cursor = page["next_cursor"].as_str().map(|s| json!(s));
    }
    ensure!(cursor.is_none(), "the last page must finish the list");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn inbox_order_uses_the_schema_and_rejects_a_stale_cursor() -> Result<()> {
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["input"] = types["note"].clone();
    types["note"]["inbox"] = json!(false);
    w.command(json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Configure the input type"})).await?;
    for kind in ["input", "note"] {
        for id in ["a-decided", "b-decided", "z-undecided"] {
            let status = if id == "z-undecided" {
                "unprocessed"
            } else {
                "processed"
            };
            w.command(json!({"op":"save_object","kind":kind,"id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"owner","body":"Input","status":status},"links":[],"reason":"Capture"})).await?;
        }
    }
    w.command(json!({"op":"save_object","kind":"input","id":"y-nested","revision":0,"parent":"domain:security","data":{"title":"Nested input","owner":"owner","body":"Input"},"links":[],"reason":"Capture"})).await?;
    let first = w.get("kind=input&parent=*&limit=1").await?;
    ensure!(first["items"][0]["address"] == "input:y-nested", "{first}");
    let mut cursor = first["next_cursor"].as_str().unwrap().to_owned();
    for address in ["input:z-undecided", "input:a-decided", "input:b-decided"] {
        let page = w
            .get(&format!("kind=input&parent=*&limit=1&cursor={cursor}"))
            .await?;
        ensure!(page["items"][0]["address"] == address, "{page}");
        cursor = page["next_cursor"].as_str().unwrap_or("").into();
    }
    ensure!(cursor.is_empty(), "the last page must finish the list");
    let ordinary = w.get("kind=note&parent=*&limit=100").await?;
    ensure!(
        ordinary["items"][0]["address"] == "note:a-decided"
            && ordinary["items"][0].get("status").is_none(),
        "the type name must not enable queue behavior: {ordinary}"
    );
    let pending = w
        .get("kind=input&parent=*&note_status=unprocessed&limit=100")
        .await?;
    ensure!(pending["items"].as_array().unwrap().len() == 2, "{pending}");
    let processed = w
        .get("kind=input&parent=*&note_status=processed&limit=1")
        .await?;
    let next = w
        .get(&format!(
            "kind=input&parent=*&note_status=processed&limit=1&cursor={}",
            processed["next_cursor"].as_str().unwrap()
        ))
        .await?;
    ensure!(
        processed["items"][0]["address"] == "input:a-decided"
            && next["items"][0]["address"] == "input:b-decided",
        "{processed} / {next}"
    );
    let stale = first["next_cursor"].as_str().unwrap();
    w.command(json!({"op":"set_note_status","target":"input:y-nested","revision":1,"status":"processed","reason":"Reviewed"})).await?;
    let response = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/objects?kind=input&parent=*&limit=1&cursor={stale}",
            w.root, w.ws
        ))
        .bearer_auth(&w.owner)
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::CONFLICT,
        "{response:?}"
    );
    ensure!(
        response.json::<Value>().await?["error"]
            == "object list changed; start again without cursor",
        "a status change must expire the cursor"
    );
    let position = w.get("kind=input&parent=*&limit=1").await?["position"]
        .as_u64()
        .unwrap();
    for key in ["2:input:z-undecided", "0:note:a-decided", "0:input:z:bad"] {
        let response = w
            .http
            .get(format!(
                "{}/api/workspaces/{}/objects?kind=input&parent=*&cursor={position}:{key}",
                w.root, w.ws
            ))
            .bearer_auth(&w.owner)
            .send()
            .await?;
        ensure!(
            response.status() == reqwest::StatusCode::CONFLICT,
            "{response:?}"
        );
    }
    let response = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/objects?kind=note&parent=*&cursor={position}:0:input:z-undecided",
            w.root, w.ws
        ))
        .bearer_auth(&w.owner)
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::CONFLICT,
        "a cursor cannot change its order mode"
    );
    Ok(())
}

// signal-in-one-inbox-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_signal_appears_in_the_inbox_list() -> Result<()> {
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let empty = w.get_as(bot, "kind=note&parent=*&limit=100").await?;
    ensure!(
        empty["items"].as_array().is_some_and(Vec::is_empty),
        "{empty}"
    );
    w.command_as(bot, json!({"op":"save_object","kind":"note","id":"first","revision":0,"parent":null,"data":{"title":"First","owner":"owner","body":"Refunds are slow"},"links":[],"reason":"Save"})).await?;
    let inbox = w.get_as(bot, "kind=note&parent=*&limit=100").await?;
    let items = inbox["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        items.len() == 1 && items[0]["address"] == "note:first",
        "{inbox}"
    );
    Ok(())
}

// signal-in-one-inbox-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_note_under_any_object_also_appears_in_the_inbox() -> Result<()> {
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    w.command_as(bot, json!({"op":"save_object","kind":"note","id":"under","revision":0,"parent":"requirement:key","data":{"title":"Under","owner":"owner","body":"About keys"},"links":[],"reason":"Save"}))
        .await?;
    let inbox = w.get_as(bot, "kind=note&parent=*&limit=100").await?;
    let items = inbox["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        items
            .iter()
            .any(|i| i["address"] == "note:under" && i["parent"] == "requirement:key"),
        "{inbox}"
    );
    Ok(())
}

// signal-becomes-spec-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_member_archives_a_signal_that_needs_no_work_with_a_reason() -> Result<()> {
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["record"] = json!({"title":"Record","role":"document","root":true,"parents":["*"]});
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Add a document outside the Inbox"})).await?;
    w.command(json!({"op":"save_object","kind":"record","id":"plain","revision":0,"parent":null,"data":{"title":"Plain record","owner":"owner"},"links":[],"reason":"Save"})).await?;
    let read = async |address: &str| -> Result<Value> {
        let result = mcp_call(
            &w,
            &w.alex,
            "joppa_objects",
            json!({"workspace":w.ws,"address":address}),
        )
        .await?;
        ensure!(result["isError"] != true, "{result}");
        Ok(serde_json::from_str(
            result["content"][0]["text"].as_str().unwrap_or("null"),
        )?)
    };
    let inbox = async || -> Result<Vec<String>> {
        let list = w.get_as(&w.alex, "kind=note&parent=*&limit=100").await?;
        Ok(list["items"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|i| i["address"].as_str().map(str::to_owned))
            .collect())
    };
    for mcp in [false, true] {
        let id = if mcp { "mcp-signal" } else { "http-signal" };
        let signal = format!("note:{id}");
        w.note(id, 0, "Dark mode", "Please add dark mode", json!([]))
            .await?;
        ensure!(
            inbox().await?.contains(&signal),
            "mcp={mcp}: not in the Inbox"
        );
        // A member stays refused for objects outside the Inbox.
        for target in ["capability:empty", "record:plain"] {
            let revision = read(target).await?["document"]["revision"].clone();
            refused_without_change(&w, mcp, &w.alex, json!({"op":"archive_object","target":target,"revision":revision,"reason":"Out of scope"}), "workspace owner or agent required").await?;
            ensure!(
                read(target).await?["document"]["present"] == true,
                "mcp={mcp} {target}"
            );
        }
        // Each other archive check still refuses the member and writes nothing.
        refused_without_change(
            &w,
            mcp,
            &w.alex,
            json!({"op":"archive_object","target":signal,"revision":1,"reason":"  "}),
            "text must not be empty",
        )
        .await?;
        refused_without_change(
            &w,
            mcp,
            &w.alex,
            json!({"op":"archive_object","target":signal,"revision":0,"reason":"Out of scope"}),
            "stale object revision; read the object again",
        )
        .await?;
        ensure!(
            read(&signal).await?["document"]["present"] == true,
            "mcp={mcp} {signal}"
        );
        let archive =
            json!({"op":"archive_object","target":signal,"revision":1,"reason":"Out of scope"});
        let reply = send_over(&w, mcp, &w.alex, &archive).await?;
        ensure!(reply.is_ok(), "mcp={mcp}: {reply:?}");
        ensure!(
            !inbox().await?.contains(&signal),
            "mcp={mcp}: still in the Inbox"
        );
        let object = read(&signal).await?;
        ensure!(
            object["document"]["present"] == false
                && object["document"]["reason"] == "Out of scope",
            "mcp={mcp}: {object}"
        );
    }
    Ok(())
}

// signal-becomes-spec-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_archives_a_signal_and_keeps_its_related_requirement_link() -> Result<()> {
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let inbox = async || -> Result<(bool, Value)> {
        let list = w.get_as(bot, "kind=note&parent=*&limit=100").await?;
        let listed = list["items"]
            .as_array()
            .is_some_and(|i| i.iter().any(|i| i["address"] == "note:signal"));
        Ok((listed, list))
    };
    w.command_as(bot, json!({"op":"save_object","kind":"note","id":"signal","revision":0,"parent":null,"data":{"title":"Dark mode","owner":"owner","body":"Please add dark mode"},"links":[],"reason":"Save"})).await?;
    let (listed, list) = inbox().await?;
    ensure!(listed, "{list}");
    w.command_as(bot, json!({"op":"save_object","kind":"requirement","id":"dark","revision":0,"parent":"capability:restrict","data":{"title":"Dark mode","body":"Offer a dark theme","owner":"owner","consumers":["customer"]},"links":[{"relation":"related","target":"note:signal"}],"reason":"Turn the signal into a requirement"})).await?;
    w.command_as(bot, json!({"op":"archive_object","target":"note:signal","revision":1,"reason":"Covered by requirement:dark"})).await?;
    let (listed, list) = inbox().await?;
    ensure!(!listed, "{list}");
    let requirement = object_as(&w, bot, "requirement:dark", None).await?;
    ensure!(
        requirement["links"].as_array().is_some_and(|l| l
            .iter()
            .any(|l| l["link"]["relation"] == "related" && l["link"]["target"] == "note:signal")),
        "{requirement}"
    );
    let signal = object_as(&w, bot, "note:signal", None).await?;
    ensure!(
        signal["document"]["present"] == false
            && signal["incoming"].as_array().is_some_and(|i| i
                .iter()
                .any(|i| i["source"] == "requirement:dark" && i["relation"] == "related")),
        "{signal}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_depends_on_link_shows_on_both_objects_over_http() -> Result<()> {
    // typed-links-ac1
    let w = world().await?;
    for id in ["a", "b"] {
        w.command_as(&w.bot, json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":null,"data":{"title":id,"owner":"owner","body":id},"links":[],"reason":"Save"})).await?;
    }
    w.command_as(&w.bot, json!({"op":"save_object","kind":"note","id":"b","revision":1,"parent":null,"data":{"title":"b","owner":"owner","body":"b"},"links":[{"target":"note:a","relation":"depends_on"}],"reason":"Save"})).await?;
    let (actor, command) = last_event(&w).await?;
    ensure!(actor == "bot", "{actor}: {command}");
    for mcp in [false, true] {
        let read = async |id: &str| -> Result<Value> {
            if mcp {
                object_as(&w, &w.bot, &format!("note:{id}"), None).await
            } else {
                w.get_as(&w.bot, &format!("address=note:{id}")).await
            }
        };
        let b = read("b").await?;
        ensure!(
            b["document"]["links"][0]["relation"] == "depends_on"
                && b["document"]["links"][0]["target"] == "note:a",
            "mcp={mcp}: {b}"
        );
        let a = read("a").await?;
        ensure!(
            a["incoming"].as_array().is_some_and(|i| i
                .iter()
                .any(|i| i["source"] == "note:b" && i["relation"] == "depends_on")),
            "mcp={mcp}: {a}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_account_without_membership_cannot_add_a_signal_over_http() -> Result<()> {
    // signal-in-one-inbox-ac3: eve learns nothing, not even that the workspace exists.
    let w = world().await?;
    let save = |ws: String| {
        let w = &w;
        async move {
            let reply = w
                .http
                .post(format!("{}/api/workspaces/{ws}/commands", w.root))
                .bearer_auth(&w.eve)
                .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"save_object","kind":"note","id":"eve","revision":0,"parent":null,"data":{"title":"Signal eve","owner":"owner","body":"Hi"},"links":[],"reason":"Intake"}}))
                .send()
                .await?;
            Ok::<_, anyhow::Error>((reply.status().as_u16(), reply.text().await?))
        }
    };
    let member_only = save(w.ws.clone()).await?;
    let absent = save(format!("absent-{}", uuid::Uuid::new_v4().simple())).await?;
    ensure!(
        member_only.0 == 404 && member_only == absent,
        "{member_only:?} vs {absent:?}"
    );
    ensure!(
        member_only.1.contains("Workspace not found"),
        "{member_only:?}"
    );
    let inbox = w.get("kind=note&parent=*&limit=100").await?;
    ensure!(
        inbox["items"].as_array().is_some_and(|i| i.is_empty()),
        "{inbox}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_account_without_membership_cannot_save_a_test_list_over_http() -> Result<()> {
    // eve learns nothing about the workspace, and the saved test list stays as it was.
    let w = world().await?;
    let list = || async {
        let status: Value = w
            .http
            .get(format!("{}/api/workspaces/{}/tests", w.root, w.ws))
            .bearer_auth(&w.owner)
            .query(&[("ac", "ac:key-rights")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok::<_, anyhow::Error>((status["tests"].clone(), status["tests_saved_by"].clone()))
    };
    let before = list().await?;
    let save = |ws: String| {
        let w = &w;
        async move {
            let reply = w
                .http
                .post(format!("{}/api/workspaces/{ws}/commands", w.root))
                .bearer_auth(&w.eve)
                .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"set_ac_tests","target":"ac:key-rights","tests":["a.test.ts > login > ok"],"reason":"Check refusal"}}))
                .send()
                .await?;
            Ok::<_, anyhow::Error>((reply.status().as_u16(), reply.text().await?))
        }
    };
    let member_only = save(w.ws.clone()).await?;
    let absent = save(format!("absent-{}", uuid::Uuid::new_v4().simple())).await?;
    ensure!(
        member_only.0 == 404 && absent.0 == 404,
        "{member_only:?} vs {absent:?}"
    );
    ensure!(
        member_only.1.contains("Workspace not found") && absent.1.contains("Workspace not found"),
        "{member_only:?} vs {absent:?}"
    );
    ensure!(member_only.1 == absent.1, "{member_only:?} vs {absent:?}");
    let after = list().await?;
    ensure!(after == before, "{before:?} vs {after:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_redacted_fragment_cannot_be_read_anywhere() -> Result<()> {
    // owner-redacts-secret-ac1
    let w = world().await?;
    w.note("leak", 0, "Leak", "the key is sk-123 for prod", json!([]))
        .await?;
    w.note("leak", 1, "Leak", "rotate sk-123 today", json!([]))
        .await?;
    ensure!(
        w.search("sk-123").await? == ["note:leak"],
        "search must find the fragment before the redaction"
    );
    w.command(json!({"op":"redact_object","target":"note:leak","fragment":"sk-123","reason":"leaked key"}))
        .await?;
    let reads = [
        w.get("address=note:leak").await?,
        w.get("address=note:leak&revision=1").await?,
        w.get("address=note:leak&revision=2").await?,
        w.get("address=note:leak&history=true").await?,
        w.get("kind=note&parent=*&limit=100").await?,
    ];
    for read in &reads {
        ensure!(!read.to_string().contains("sk-123"), "{read}");
    }
    ensure!(
        reads[1]["document"]["data"]["body"] == "the key is [redacted] for prod",
        "{}",
        reads[1]
    );
    let found = w.search("sk-123").await?;
    ensure!(found.is_empty(), "{found:?}");
    let state: String = w
        .pool
        .get()
        .await?
        .query_one(
            "SELECT value::text FROM joppa.state_entries WHERE workspace=$1 AND section='redactions' AND key='note:leak'",
            &[&w.ws],
        )
        .await?
        .get(0);
    ensure!(
        state.contains("leaked key") && state.contains("owner") && !state.contains("sk-123"),
        "{state}"
    );
    // A rebuild from the journal replays the redaction and agrees with the stored revisions.
    w.pool
        .get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&w.ws])
        .await?;
    store::backfill_board(&w.pool).await?;
    let rebuilt = w.get("address=note:leak&revision=1").await?;
    ensure!(
        rebuilt["document"]["data"]["body"] == "the key is [redacted] for prod",
        "{rebuilt}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_item_id_and_an_address_find_their_object_first() -> Result<()> {
    // lookup-by-id-ac1, lookup-by-id-ac2
    let w = world().await?;
    w.note("first", 0, "Passkey plan", "passkey for first", json!([]))
        .await?;
    w.note(
        "second",
        0,
        "Passkey rollout",
        "passkey for second",
        json!([]),
    )
    .await?;
    let item_id = w.get("address=note:second").await?["item_id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    ensure!(!item_id.is_empty(), "the note needs an item ID");
    let by_id = w.first_exact(&item_id).await?;
    ensure!(
        by_id.as_deref() == Some("note:second"),
        "{item_id}: {by_id:?}"
    );
    let by_address = w.first_exact("note:second").await?;
    ensure!(
        by_address.as_deref() == Some("note:second"),
        "{by_address:?}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_agent_bot_finds_an_object_first_by_its_current_and_kept_item_ids() -> Result<()> {
    // lookup-by-id-ac1
    let w = world().await?;
    w.note("first", 0, "Passkey plan", "passkey for first", json!([]))
        .await?;
    w.note(
        "second",
        0,
        "Passkey rollout",
        "passkey for second",
        json!([]),
    )
    .await?;
    let current = w.get("address=note:second").await?["item_id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    ensure!(!current.is_empty(), "the note needs an item ID");
    bot_finds_first(&w, &current).await?;
    let prefix = format!("renamed-{}", uuid::Uuid::new_v4().simple());
    w.command(json!({"op":"rename_workspace","title":"Spec over HTTP","reason":"New prefix","reference_prefix":prefix}))
        .await?;
    let renamed = w.get("address=note:second").await?["item_id"].clone();
    ensure!(
        renamed.as_str().is_some_and(|id| id.starts_with(&prefix)),
        "{renamed}"
    );
    bot_finds_first(&w, &current).await
}

/// The agent bot's search for an item ID returns note:second first, with that ID as an exact match.
async fn bot_finds_first(w: &World, query: &str) -> Result<()> {
    let found = mcp_value(w, "joppa_search", json!({"workspace":w.ws,"query":query})).await?;
    let first = &found["items"][0];
    ensure!(
        first["kind"] == "note"
            && first["id"] == "second"
            && first["system_id"] == query
            && first["exact_id"] == true,
        "{query}: {found}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_agent_bot_finds_an_object_first_by_its_address() -> Result<()> {
    // lookup-by-id-ac2
    let w = world().await?;
    w.note("first", 0, "Passkey plan", "passkey for first", json!([]))
        .await?;
    w.note(
        "second",
        0,
        "Passkey rollout",
        "passkey for second",
        json!([]),
    )
    .await?;
    let query = "note:second";
    let found = mcp_value(&w, "joppa_search", json!({"workspace":w.ws,"query":query})).await?;
    let first = &found["items"][0];
    ensure!(
        first["kind"] == "note" && first["id"] == "second" && first["exact_id"] == true,
        "{query}: {found}"
    );
    for (name, arguments) in [
        ("joppa_search", json!({"workspace":w.ws,"query":query})),
        ("joppa_objects", json!({"workspace":w.ws,"address":query})),
    ] {
        let denied = mcp_call(&w, &w.eve, name, arguments).await?;
        ensure!(
            denied["isError"] == true && !denied.to_string().contains("Passkey rollout"),
            "{name} {query}: {denied}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_signal_keeps_its_channel_author_and_time() -> Result<()> {
    // signal-records-source-ac1
    let w = world().await?;
    let source = json!({"channel_type":"chat","channel_name":"support","author":"maria","at":"2026-10-09T10:00:00Z"});
    w.command_as(&w.bot, json!({"op":"save_object","kind":"note","id":"chat","revision":0,"parent":null,"data":{"title":"Refunds are slow","owner":"owner","body":"My refund takes a week","source":source},"links":[],"reason":"Intake"}))
        .await?;
    let read = w.get("address=note:chat").await?;
    ensure!(read["document"]["data"]["source"] == source, "{read}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_granted_agent_write_records_its_own_journal_identity() -> Result<()> {
    // agent-acts-within-grant-ac1
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let request_id = uuid::Uuid::new_v4().to_string();
    let command = json!({"op":"save_object","kind":"note","id":"agent-author","revision":0,"parent":null,"data":{"title":"The granted agent writes as itself","owner":"owner","body":"The owner and author have separate meanings","source":{"actor":"business source"}},"reason":"Record the authenticated agent author"});
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(bot)
        .json(&json!({"request_id":request_id,"command":command}))
        .send()
        .await?;
    ensure!(
        response.status() == reqwest::StatusCode::OK,
        "{}",
        response.status()
    );
    let saved: Value = response.json().await?;
    let position = saved["position"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("The write returned no journal position: {saved}"))?;
    let conn = w.pool.get().await?;
    let events = conn.query("SELECT position,actor,command::text FROM joppa.events WHERE workspace=$1 AND request_id=$2", &[&w.ws,&request_id]).await?;
    ensure!(events.len() == 1, "The write has {} events", events.len());
    ensure!(
        events[0].get::<_, i64>(0) as u64 == position && events[0].get::<_, String>(1) == "bot",
        "The saved event has a different position or author"
    );
    let stored: Value = serde_json::from_str(events[0].get(2))?;
    ensure!(
        stored["op"] == "save_object" && stored["kind"] == "note" && stored["id"] == "agent-author",
        "{stored}"
    );
    let journal = store::read(&w.pool, &w.ws, "owner", &store::Scope::full()).await?;
    let events: Vec<_> = journal["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["position"] == position)
        .collect();
    ensure!(
        events.len() == 1,
        "The journal lost or duplicated the write"
    );
    ensure!(
        events[0]["actor"] == "bot" && events[0]["actor_type"] == "agent",
        "{:?}",
        events[0]
    );
    let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":w.ws,"address":"note:agent-author"}}})).send().await?;
    ensure!(
        response.status() == reqwest::StatusCode::OK,
        "{}",
        response.status()
    );
    let response: Value = response.json().await?;
    ensure!(response["result"]["isError"] != true, "{response}");
    let object: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("null"),
    )?;
    ensure!(
        object["document"]["actor"] == "bot" && object["document"]["actor_type"] == "agent",
        "{object}"
    );
    ensure!(
        object["document"]["data"]["owner"] == "owner"
            && object["document"]["data"]["source"]["actor"] == "business source",
        "The identity changed business data: {object}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn bot1_and_bot2_saves_record_their_own_journal_actor() -> Result<()> {
    // agent-own-identity-ac1
    let w = world().await?;
    let mut tokens = Vec::new();
    for agent in ["bot1", "bot2"] {
        let token = store::create_agent_token(&w.pool, &w.ws, "owner", agent, None).await?;
        let token = token["token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No token for {agent}: {token}"))?;
        tokens.push(token.to_string());
    }
    ensure!(tokens[0] != tokens[1], "bot1 and bot2 share a token");
    let mut saves = Vec::new();
    for (agent, token) in ["bot1", "bot2"].into_iter().zip(&tokens) {
        let request_id = uuid::Uuid::new_v4().to_string();
        let command = json!({"op":"save_object","kind":"note","id":format!("{agent}-note"),"revision":0,"parent":null,"data":{"title":format!("{agent} saves its own note"),"owner":"owner","body":"Each agent writes as itself"},"reason":"Record the agent author"});
        let response = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(token)
            .json(&json!({"request_id":request_id,"command":command}))
            .send()
            .await?;
        ensure!(
            response.status() == reqwest::StatusCode::OK,
            "{agent} request {request_id}: {}",
            response.status()
        );
        let saved: Value = response.json().await?;
        let position = saved["position"].as_u64().ok_or_else(|| {
            anyhow::anyhow!("{agent} request {request_id} returned no journal position: {saved}")
        })?;
        saves.push((agent, request_id, position));
    }
    ensure!(
        saves[0].1 != saves[1].1 && saves[0].2 != saves[1].2,
        "{saves:?}"
    );
    let conn = w.pool.get().await?;
    let journal = store::read(&w.pool, &w.ws, "owner", &store::Scope::full()).await?;
    for (agent, request_id, position) in &saves {
        let events = conn
            .query(
                "SELECT position,actor FROM joppa.events WHERE workspace=$1 AND request_id=$2",
                &[&w.ws, request_id],
            )
            .await?;
        let stored: Vec<(i64, String)> = events.iter().map(|e| (e.get(0), e.get(1))).collect();
        ensure!(
            stored.len() == 1 && stored[0].0 as u64 == *position && stored[0].1 == *agent,
            "{agent} request {request_id} at position {position} stored events {stored:?}"
        );
        let history: Vec<_> = journal["history"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("The journal has no history"))?
            .iter()
            .filter(|event| event["position"] == *position)
            .collect();
        ensure!(
            history.len() == 1
                && history[0]["actor"] == *agent
                && history[0]["actor_type"] == "agent",
            "{agent} request {request_id} at position {position} history events {history:?}"
        );
        for other in ["bot1", "bot2", "owner"]
            .into_iter()
            .filter(|other| other != agent)
        {
            ensure!(
                stored[0].1 != other && history[0]["actor"] != other,
                "{agent} request {request_id} names {other}: {stored:?} {history:?}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_question_reaches_its_named_person_and_keeps_the_input_open() -> Result<()> {
    // agent-asks-ac1
    async fn read(w: &World, address: &str, revision: Option<u32>) -> Result<Value> {
        let mut arguments = json!({"workspace":w.ws,"address":address});
        if let Some(revision) = revision {
            arguments["revision"] = json!(revision);
        }
        let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.maria).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":arguments}})).send().await?;
        ensure!(response.status().is_success(), "{}", response.status());
        let response: Value = response.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        Ok(serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?)
    }
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot", None).await?;
    let bot = token["token"].as_str().unwrap();
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["input"] = types["note"].clone();
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check a question on a configured inbox type"})).await?;
    for kind in ["note", "input"] {
        let address = format!("{kind}:unclear");
        w.command_as(bot,json!({"op":"save_object","kind":kind,"id":"unclear","revision":0,"parent":null,"data":{"title":"Which plan needs this change","owner":"owner","body":"Clarify the plan before changing the specification","source":{"author":"alex"}},"reason":"Record an unclear input"})).await?;
        let command = json!({"op":"decide_note","target":address,"revision":1,"outcome":"needs_information","reason":"Ask the person who can clarify the plan","question":"Which plan?","recipient":"maria"});
        let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":command}}})).send().await?;
        ensure!(response.status().is_success(), "{}", response.status());
        let response: Value = response.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        let asked = read(&w, &address, None).await?;
        ensure!(
            asked["question"] == json!({"recipient":"maria","text":"Which plan?"}),
            "{asked}"
        );
        ensure!(
            asked["document"]["data"]["status"] == "unprocessed"
                && asked["document"]["data"]["source"]["author"] == "alex",
            "{asked}"
        );
        let response = w
            .http
            .get(format!("{}/api/workspaces/{}/objects", w.root, w.ws))
            .query(&[("address", address.as_str())])
            .bearer_auth(&w.maria)
            .send()
            .await?;
        ensure!(response.status().is_success(), "{}", response.status());
        ensure!(
            response.json::<Value>().await? == asked,
            "HTTP and MCP returned different questions"
        );
        let response = w.http.post(format!("{}/api/workspaces/{}/commands",w.root,w.ws)).bearer_auth(bot).json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"decide_note","target":address,"revision":2,"outcome":"needs_information","reason":"Test an invalid question recipient","question":"Which plan?","recipient":"bot"}})).send().await?;
        ensure!(
            response.status() == reqwest::StatusCode::CONFLICT,
            "{}",
            response.status()
        );
        ensure!(
            response.json::<Value>().await?["error"] == "a human member is required",
            "An agent was accepted as the named person"
        );
        ensure!(
            read(&w, &address, None).await?["document"] == asked["document"],
            "The refused recipient changed the input"
        );
        w.command_as(bot,json!({"op":"decide_note","target":address,"revision":2,"outcome":"changed_spec","reason":"The specification now states the plan","links":["ac:key-rights"]})).await?;
        let processed = read(&w, &address, None).await?;
        ensure!(
            processed["document"]["data"]["status"] == "processed"
                && processed.get("question").is_none(),
            "{processed}"
        );
        let old = read(&w, &address, Some(2)).await?;
        ensure!(
            old["question"] == asked["question"]
                && old["document"]["data"]["status"] == "unprocessed",
            "The saved question changed: {old}"
        );
        w.command_as(bot,json!({"op":"decide_note","target":address,"revision":3,"outcome":"needs_information","reason":"Keep the existing author routing","question":"Which plan?"})).await?;
        let default = read(&w, &address, None).await?;
        ensure!(
            default["question"]["recipient"] == "alex"
                && default["document"]["data"]["status"] == "unprocessed",
            "{default}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_file_version_lists_first_and_both_versions_download() -> Result<()> {
    use anyhow::Context;
    // files-keep-versions-ac1
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["file_target"] = types["consumer"].clone();
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check files on a configured object type"})).await?;
    w.command(json!({"op":"save_object","kind":"file_target","id":"spec","revision":0,"parent":null,"data":{"title":"Specification files","owner":"owner"},"reason":"Create a configured file target"})).await?;
    for address in ["requirement:key", "file_target:spec"] {
        ensure!(w.get(&format!("address={address}")).await?["files"] == json!([]));
        let contents = [
            b"Original specification".as_slice(),
            b"Revised specification".as_slice(),
        ];
        let mut hashes = Vec::new();
        for (id, bytes) in ["a-original", "z-revised"].into_iter().zip(contents) {
            let response = w
                .http
                .post(format!("{}/api/workspaces/{}/blobs", w.root, w.ws))
                .bearer_auth(&w.alex)
                .body(bytes.to_vec())
                .send()
                .await?;
            ensure!(
                response.status() == reqwest::StatusCode::OK,
                "{}",
                response.status()
            );
            let blob: Value = response.json().await?;
            let hash = blob["hash"].as_str().unwrap().to_owned();
            w.command_as(&w.alex, json!({"op":"attach","target":address,"id":id,"name":"spec.pdf","content_type":"application/pdf","size":bytes.len(),"hash":hash})).await?;
            hashes.push(hash);
        }
        let object = w.get(&format!("address={address}")).await?;
        let files = object["files"]
            .as_array()
            .context("ordered files list missing")?;
        ensure!(files.len() == 2, "{object}");
        ensure!(
            files[0]["id"] == "z-revised" && files[1]["id"] == "a-original",
            "{files:?}"
        );
        for file in files {
            ensure!(
                file["name"] == "spec.pdf" && file["actor"] == "alex",
                "{file}"
            );
            ensure!(file["created_at"] == file["at"], "{file}");
        }
        let created = |file: &Value| {
            chrono::DateTime::parse_from_str(
                file["created_at"].as_str().unwrap_or(""),
                "%Y-%m-%d %H:%M:%S%.f%#z",
            )
        };
        ensure!(created(&files[0])? > created(&files[1])?, "{files:?}");
        let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.alex).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":w.ws,"address":address}}})).send().await?;
        ensure!(
            response.status() == reqwest::StatusCode::OK,
            "{}",
            response.status()
        );
        let response: Value = response.json().await?;
        ensure!(response["result"]["isError"] != true, "{response}");
        let mcp: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or("null"),
        )?;
        ensure!(
            mcp == object,
            "HTTP and MCP returned different file versions"
        );
        for (file, bytes) in files.iter().zip(contents.into_iter().rev()) {
            let response = w
                .http
                .get(format!(
                    "{}/api/workspaces/{}/blobs/{}",
                    w.root,
                    w.ws,
                    file["hash"].as_str().unwrap()
                ))
                .bearer_auth(&w.alex)
                .send()
                .await?;
            ensure!(
                response.status() == reqwest::StatusCode::OK,
                "{}",
                response.status()
            );
            ensure!(response.headers()[reqwest::header::CONTENT_TYPE] == "application/pdf");
            ensure!(
                response.bytes().await?.as_ref() == bytes,
                "A file version changed"
            );
        }
        w.command_as(&w.alex, json!({"op":"detach","target":address,"id":"z-revised","reason":"Hide the revised attachment"})).await?;
        let detached = w.get(&format!("address={address}")).await?;
        ensure!(
            detached["files"].as_array().unwrap().len() == 1
                && detached["files"][0]["id"] == "a-original",
            "{detached}"
        );
        ensure!(detached["annotations"]["attachments"]["z-revised"]["removed"] == true);
        let response = w
            .http
            .get(format!(
                "{}/api/workspaces/{}/blobs/{}",
                w.root, w.ws, hashes[0]
            ))
            .bearer_auth(&w.alex)
            .send()
            .await?;
        ensure!(response.status() == reqwest::StatusCode::OK);
        ensure!(response.bytes().await?.as_ref() == contents[0]);
        let mut data = object["document"]["data"].clone();
        data["title"] = json!(format!("{} revised", data["title"].as_str().unwrap()));
        w.command_as(&w.alex, json!({"op":"save_object","kind":object["kind"],"id":object["id"],"revision":1,"parent":object["document"]["parent"],"data":data,"reason":"Create a later document revision"})).await?;
        let historical = w.get(&format!("address={address}&revision=1")).await?;
        ensure!(
            historical["historical"] == true && historical.get("files").is_none(),
            "A saved document exposes the current files: {historical}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn configured_type_question_is_available_to_an_agent_over_mcp() -> Result<()> {
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    let question = "Which observation changes the current contract?";
    types["observation"] = json!({"title":"Observation","role":"document","root":true,"parents":["*"],"question":question});
    types["record"] = json!({"title":"Record","role":"document","root":true,"parents":["*"]});
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Configure the authoring question"})).await?;
    let call = |name: &str, arguments: Value| {
        let name = name.to_owned();
        let w = &w;
        async move {
            let reply = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.bot)
                .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}))
                .send().await?;
            ensure!(reply.status().is_success(), "{}", reply.status());
            let reply: Value = reply.json().await?;
            ensure!(
                reply.get("error").is_none() && reply["result"]["isError"] != true,
                "{reply}"
            );
            let value: Value =
                serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap())?;
            Ok::<_, anyhow::Error>(value)
        }
    };
    let help = call("joppa_help", json!({"workspace":w.ws})).await?;
    let tree = call("joppa_tree", json!({"workspace":w.ws})).await?;
    ensure!(help["tree"] == tree, "{help} vs {tree}");
    ensure!(
        tree["schema"]["types"]["observation"]["question"] == question,
        "{tree}"
    );
    ensure!(
        tree["schema"]["types"]["record"].get("question").is_none(),
        "{tree}"
    );
    let generic_help = call("joppa_help", json!({})).await?;
    ensure!(
        generic_help.get("tree").is_none() && generic_help["operations"].is_object(),
        "{generic_help}"
    );
    for kind in ["observation", "record"] {
        let request_id = uuid::Uuid::new_v4().to_string();
        let mut command = json!({"op":"save_object","kind":kind,"id":"one","revision":0,"parent":null,"data":{"title":"Agent observation","owner":"owner","description":"An observed failure changes the contract."},"reason":"Record the observation"});
        if kind == "observation" {
            let error = format!(
                "Check that the title and description answer: {question}. Rewrite them if they do not. Then resend with question_checked"
            );
            let conn = w.pool.get().await?;
            let before: i64 = conn
                .query_one(
                    "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
                    &[&w.ws],
                )
                .await?
                .get(0);
            for checked in [None, Some("A different question")] {
                if let Some(checked) = checked {
                    command["question_checked"] = json!(checked);
                }
                let response = w
                    .http
                    .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
                    .bearer_auth(&w.bot)
                    .json(&json!({"request_id":request_id,"command":command}))
                    .send()
                    .await?;
                ensure!(response.status().as_u16() == 422, "{}", response.status());
                ensure!(
                    response.json::<Value>().await?["error"] == error,
                    "wrong question error"
                );
                let response = w.http.post(format!("{}/mcp", w.root)).bearer_auth(&w.bot)
                    .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":request_id,"command":command}}})).send().await?;
                ensure!(response.status().is_success(), "{}", response.status());
                let refused: Value = response.json().await?;
                ensure!(
                    refused["result"]["isError"] == true
                        && refused["result"]["content"][0]["text"] == error,
                    "{refused}"
                );
            }
            let after: i64 = conn
                .query_one(
                    "SELECT COUNT(*) FROM joppa.events WHERE workspace=$1",
                    &[&w.ws],
                )
                .await?
                .get(0);
            ensure!(before == after, "a refused question appended events");
            let revisions: i64 = conn.query_one("SELECT COUNT(*) FROM joppa.object_revisions WHERE workspace=$1 AND address='observation:one'", &[&w.ws]).await?.get(0);
            let receipts: i64 = conn.query_one("SELECT COUNT(*) FROM joppa.command_receipts WHERE workspace=$1 AND request_id=$2", &[&w.ws,&request_id]).await?.get(0);
            ensure!(
                revisions == 0 && receipts == 0,
                "a refused question wrote revisions or a receipt"
            );
            command["question_checked"] = json!(question);
        }
        let saved = call(
            "joppa_command",
            json!({"workspace":w.ws,"request_id":request_id,"command":command}),
        )
        .await?;
        ensure!(saved["object"]["revision"] == 1, "{saved}");
        if kind == "observation" {
            ensure!(saved["object"]["question"] == question, "{saved}");
        } else {
            ensure!(saved["object"].get("question").is_none(), "{saved}");
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn legacy_revision_cannot_bypass_a_configured_type_question() -> Result<()> {
    let w = world().await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["requirement"]["question"] = json!("Which behavior does the product need?");
    types["domain"]["question"] = json!("Which product area does this object describe?");
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Check the requirement question"})).await?;
    let response = w.http.post(format!("{}/api/workspaces/{}/commands", w.root, w.ws)).bearer_auth(&w.owner)
        .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"domain","id":"new","title":"New product area","authority":"owner"}})).send().await?;
    ensure!(response.status().as_u16() == 422, "{}", response.status());
    ensure!(
        response.json::<Value>().await?["error"]
            == "This type has a question. Use save_object with question_checked",
        "wrong creation question error"
    );
    let exists: bool = w.pool.get().await?.query_one("SELECT EXISTS(SELECT 1 FROM joppa.objects WHERE workspace=$1 AND address='domain:new')", &[&w.ws]).await?.get(0);
    ensure!(!exists, "a refused legacy creation left an object");
    let original = w.get("address=requirement:key").await?;
    let command = json!({"op":"revise","req":"key","expected_revision":1,"capability":"restrict","title":"Updated rights","body":"Updated scope","owner":"owner","consumers":["customer"],"reason":"Revise the legacy contract","acs":[{"id":"key-rights","text":"Only read and verify before launch"}]});
    let request_id = uuid::Uuid::new_v4().to_string();
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id":request_id,"command":command}))
        .send()
        .await?;
    ensure!(response.status().as_u16() == 422, "{}", response.status());
    ensure!(
        response.json::<Value>().await?["error"]
            == "This type has a question. Use save_object with question_checked",
        "wrong legacy question error"
    );
    ensure!(
        w.get("address=requirement:key").await?["document"] == original["document"],
        "a refused legacy revision changed the object"
    );
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["requirement"]
        .as_object_mut()
        .unwrap()
        .remove("question");
    w.command(json!({"op":"set_tree_schema","revision":schema["current_revision"],"types":types,"reason":"Keep the old path for types without a question"})).await?;
    let response = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id":request_id,"command":command}))
        .send()
        .await?;
    ensure!(response.status().is_success(), "{}", response.text().await?);
    let updated = w.get("address=requirement:key").await?;
    ensure!(
        updated["document"]["data"]["title"] == "Updated rights"
            && updated["current_revision"] == original["current_revision"].as_u64().unwrap() + 1,
        "{updated}"
    );
    Ok(())
}

#[path = "common/claim_time.rs"]
mod claim_time;

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn permanent_claims_and_heartbeat_refusals_agree_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    claim_time::install(&w.pool).await?;
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["job"] = json!({"title":"Jobs","role":"task","root":true,"parents":["*"]});
    w.command(json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Use configured work"})).await?;
    w.command(json!({"op":"save_object","kind":"job","id":"one","revision":0,"parent":null,"data":{"title":"Implement","owner":"owner","contract":"Implement and check the contract","purpose":"implementation"},"reason":"Create work"})).await?;
    let response=w.http.post(format!("{}/api/workspaces/{}/commands",w.root,w.ws)).bearer_auth(&w.bot).json(&json!({"request_id":format!("test-claim-time-86400000-{}",uuid::Uuid::new_v4().simple()),"command":{"op":"claim_task","id":"job:one","revision":1}})).send().await?;
    ensure!(response.status().is_success(), "{}", response.text().await?);
    let read_mcp = async |name: &str, arguments: Value| -> Result<Value> {
        let reply:Value=w.http.post(format!("{}/mcp",w.root)).bearer_auth(&w.bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}})).send().await?.json().await?;
        ensure!(reply["result"]["isError"] != true, "{reply}");
        Ok(serde_json::from_str(
            reply["result"]["content"][0]["text"].as_str().unwrap(),
        )?)
    };
    let before = w.get("address=job:one").await?;
    for view in ["all", "mine", "available"] {
        let http: Value = w
            .http
            .get(format!(
                "{}/api/workspaces/{}/tasks?view={view}",
                w.root, w.ws
            ))
            .bearer_auth(&w.bot)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mcp = read_mcp("joppa_tasks", json!({"workspace":w.ws,"view":view})).await?;
        ensure!(http["tasks"] == mcp["tasks"], "HTTP {http} / MCP {mcp}");
        if view == "available" {
            ensure!(http["total"] == 0, "held work is unavailable");
        } else {
            ensure!(
                http["tasks"][0]["status"] == "in_progress"
                    && http["tasks"][0]["assignee"] == "bot",
                "{http}"
            );
        }
    }
    let command = json!({"op":"heartbeat_task","id":"job:one","revision":2});
    let http: Value = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.bot)
        .json(&json!({"request_id":"refused-http","command":command}))
        .send()
        .await?
        .json()
        .await?;
    ensure!(
        http["error"] == "heartbeat_task is no longer supported",
        "{http}"
    );
    let mcp:Value=w.http.post(format!("{}/mcp",w.root)).bearer_auth(&w.bot).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":w.ws,"request_id":"refused-mcp","command":command}}})).send().await?.json().await?;
    ensure!(
        mcp["result"]["isError"] == true
            && mcp["result"]["content"][0]["text"] == "heartbeat_task is no longer supported",
        "{mcp}"
    );
    ensure!(
        before == w.get("address=job:one").await?,
        "refusals change no state"
    );
    let help = read_mcp("joppa_help", json!({"workspace":w.ws})).await?;
    ensure!(
        !help["operations"]
            .as_object()
            .unwrap()
            .contains_key("heartbeat_task"),
        "help does not advertise heartbeat writes"
    );
    store::check_journal(&w.pool).await?;
    Ok(())
}

/// One MCP tools/call; returns the JSON-RPC result.
async fn mcp_call(w: &World, token: &str, name: &str, arguments: Value) -> Result<Value> {
    let reply: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(reply.get("error").is_none(), "{reply}");
    Ok(reply["result"].clone())
}

/// Counts of everything a write can change: journal events, tokens, files and reports.
async fn stored_counts(w: &World) -> Result<Vec<i64>> {
    let conn = w.pool.get().await?;
    let row = conn
        .query_one(
            "SELECT (SELECT count(*) FROM joppa.events WHERE workspace=$1),
                    (SELECT count(*) FROM joppa.agent_tokens),
                    (SELECT count(*) FROM joppa.agent_token_revocations),
                    (SELECT count(*) FROM joppa.blobs),
                    (SELECT count(*) FROM joppa.test_reports),
                    (SELECT count(*) FROM joppa.command_receipts)",
            &[&w.ws],
        )
        .await?;
    Ok((0..6).map(|i| row.get(i)).collect())
}

/// Committed rows that a write can change, each in a stable order: the workspace journal and
/// its position, current state with members, receipts, tokens, reports and all file blobs.
async fn saved_rows(w: &World) -> Result<Value> {
    let conn = w.pool.get().await?;
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
        "task_queue",
        "agent_tokens",
        "test_reports",
    ] {
        let rows: String = conn
            .query_one(
                &format!("SELECT COALESCE(jsonb_agg(to_jsonb(t) ORDER BY to_jsonb(t)),'[]')::text FROM joppa.{table} t WHERE workspace=$1"),
                &[&w.ws],
            )
            .await?
            .get(0);
        saved[table] = serde_json::from_str(&rows)?;
    }
    let row = conn
        .query_one(
            "SELECT (SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)),'[]')::text
                       FROM joppa.agent_token_revocations r JOIN joppa.agent_tokens t ON t.id=r.token_id WHERE t.workspace=$1),
                    (SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)),'[]')::text
                       FROM joppa.test_results r JOIN joppa.test_reports t ON t.id=r.report_id WHERE t.workspace=$1),
                    (SELECT COALESCE(jsonb_agg(jsonb_build_array(hash,size,created_at) ORDER BY hash),'[]')::text FROM joppa.blobs),
                    (SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1)",
            &[&w.ws],
        )
        .await?;
    for (i, name) in ["agent_token_revocations", "test_results", "blobs"]
        .into_iter()
        .enumerate()
    {
        saved[name] = serde_json::from_str(row.get(i))?;
    }
    saved["position"] = json!(row.get::<_, i64>(3));
    Ok(saved)
}

/// Every joppa_objects page with parent=*, following next_cursor to the end.
async fn all_object_pages(w: &World, token: &str) -> Result<Vec<Value>> {
    let mut items = vec![];
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let mut arguments = json!({"workspace":w.ws,"parent":"*","limit":2});
        if let Some(cursor) = &cursor {
            arguments["cursor"] = json!(cursor);
        }
        let result = mcp_call(w, token, "joppa_objects", arguments).await?;
        ensure!(result["isError"] != true, "{result}");
        let page: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap_or(""))?;
        items.extend(page["items"].as_array().cloned().unwrap_or_default());
        pages += 1;
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    ensure!(pages > 1, "the fixture must fill more than one page");
    Ok(items)
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_read_only_agent_token_reads_the_workspace() -> Result<()> {
    // read-only-agent-token-ac1
    let w = world().await?;
    for id in ["first", "second", "third"] {
        w.note(id, 0, id, "Input", json!([])).await?;
    }
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/agent-tokens", w.root, w.ws))
        .bearer_auth(&w.owner)
        .json(&json!({"agent":"planner-reader","access":"read"}))
        .send()
        .await?;
    ensure!(reply.status().is_success(), "{}", reply.text().await?);
    let created: Value = reply.json().await?;
    ensure!(created["access"] == "read", "{created}");
    let reader = created["token"].as_str().unwrap().to_owned();
    ensure!(reader.starts_with("jpt_"), "{created}");
    let role: Option<String> = w
        .pool
        .get()
        .await?
        .query_opt(
            "SELECT value#>>'{}' FROM joppa.state_entries WHERE workspace=$1 AND section='members' AND key='planner-reader'",
            &[&w.ws],
        )
        .await?
        .map(|row| row.get(0));
    ensure!(role.as_deref() == Some("agent"), "{role:?}");
    let grant = store::token_actor(&w.pool, &reader).await?.unwrap();
    ensure!(
        grant.agent == "planner-reader"
            && grant.workspace == w.ws
            && grant.access == store::Access::Read,
        "{grant:?}"
    );

    // Every allowlisted tool reads and stores nothing.
    let before = stored_counts(&w).await?;
    for (name, arguments) in [
        ("joppa_list", json!({})),
        (
            "joppa_search",
            json!({"workspace":w.ws,"query":"Key rights"}),
        ),
        ("joppa_read", json!({"workspace":w.ws})),
        (
            "joppa_tests",
            json!({"workspace":w.ws,"ac":"ac:key-rights"}),
        ),
        ("joppa_tree", json!({"workspace":w.ws})),
        ("joppa_objects", json!({"workspace":w.ws,"parent":"*"})),
        ("joppa_tasks", json!({"workspace":w.ws})),
        ("joppa_help", json!({})),
        ("joppa_help", json!({"workspace":w.ws})),
        ("joppa_model_docs", json!({})),
    ] {
        let result = mcp_call(&w, &reader, name, arguments).await?;
        ensure!(result["isError"] != true, "{name}: {result}");
    }
    let owner_items = all_object_pages(&w, &w.owner).await?;
    let reader_items = all_object_pages(&w, &reader).await?;
    ensure!(
        owner_items == reader_items,
        "{owner_items:?} / {reader_items:?}"
    );
    ensure!(stored_counts(&w).await? == before, "a read stored data");

    // Write tools and unknown tools are refused before any store call.
    for (name, arguments) in [
        (
            "joppa_command",
            json!({"workspace":w.ws,"request_id":"read-only-command","command":{"op":"save_object","kind":"note","id":"refused","revision":0,"parent":null,"data":{"title":"Refused","owner":"owner","body":"Input"},"reason":"Save"}}),
        ),
        (
            "joppa_batch",
            json!({"workspace":w.ws,"request_id":"read-only-batch","operations":[{"command":{"op":"save_object","kind":"note","id":"refused","revision":0,"parent":null,"data":{"title":"Refused","owner":"owner","body":"Input"},"reason":"Save"}}]}),
        ),
        ("joppa_unknown", json!({"workspace":w.ws})),
    ] {
        let result = mcp_call(&w, &reader, name, arguments).await?;
        ensure!(
            result["isError"] == true
                && result["content"][0]["text"]
                    .as_str()
                    .is_some_and(|t| t == "read-only token"),
            "{name}: {result}"
        );
    }
    ensure!(
        stored_counts(&w).await? == before,
        "a refused tool stored data"
    );

    // Every HTTP route that changes state refuses the read grant.
    let id = created["id"].as_i64().unwrap();
    let api = format!("{}/api/workspaces/{}", w.root, w.ws);
    for (path, body) in [
        ("/commands".to_owned(), json!({"request_id":"read-only-http","command":{"op":"save_object","kind":"note","id":"refused","revision":0,"parent":null,"data":{"title":"Refused","owner":"owner","body":"Input"},"reason":"Save"}}).to_string()),
        ("/order".to_owned(), json!({"request_id":"read-only-order","kind":"note","parent":null,"expected":[],"ids":[]}).to_string()),
        ("/agent-tokens".to_owned(), json!({"agent":"another-reader"}).to_string()),
        (format!("/agent-tokens/{id}/revoke"), String::new()),
        ("/blobs".to_owned(), "file body".to_owned()),
        ("/reports".to_owned(), "{}".to_owned()),
    ] {
        let reply = w
            .http
            .post(format!("{api}{path}"))
            .bearer_auth(&reader)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await?;
        let status = reply.status().as_u16();
        let text = reply.text().await?;
        ensure!(
            status == 403
                && serde_json::from_str::<Value>(&text)
                    .is_ok_and(|v| v["error"] == "read-only token"),
            "{path}: {status} {text}"
        );
    }
    ensure!(
        stored_counts(&w).await? == before,
        "a refused route stored data"
    );
    let read = w
        .http
        .get(format!("{api}/objects?parent=*"))
        .bearer_auth(&reader)
        .send()
        .await?;
    ensure!(read.status().is_success(), "{}", read.status());

    // A revoked read token is refused.
    let reply = w
        .http
        .post(format!("{api}/agent-tokens/{id}/revoke"))
        .bearer_auth(&w.owner)
        .send()
        .await?;
    let status = reply.status().as_u16();
    let text = reply.text().await?;
    ensure!(status == 200, "revoke: {status} {text}");
    let reply = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&reader)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_tree","arguments":{"workspace":w.ws}}}))
        .send()
        .await?;
    let status = reply.status().as_u16();
    let text = reply.text().await?;
    ensure!(
        status == 401
            && serde_json::from_str::<Value>(&text).is_ok_and(|v| v["error"] == "token revoked"),
        "{status} {text}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_read_only_agent_token_cannot_write() -> Result<()> {
    // read-only-agent-token-ac2
    let w = world().await?;
    /// One API POST; returns the status code and the JSON reply.
    async fn post(w: &World, token: &str, path: &str, body: &str) -> Result<(u16, Value)> {
        let reply = w
            .http
            .post(format!("{}/api/workspaces/{}{path}", w.root, w.ws))
            .bearer_auth(token)
            .header("content-type", "application/json")
            .body(body.to_owned())
            .send()
            .await?;
        let status = reply.status().as_u16();
        let text = reply.text().await?;
        Ok((status, serde_json::from_str(&text).unwrap_or(json!(text))))
    }

    // A claimable task, an object to comment on and the current tree schema revision.
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"claimable","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the key rights","owner":"owner","contract":"Implement the key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Create claimable work"})).await?;
    w.note("comment-target", 0, "Comment target", "Input", json!([]))
        .await?;
    let tree = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = tree["schema"]["types"].clone();
    types["job"] = types["task"].clone();
    types["job"]["title"] = json!("Job");

    // Owner S issues the read token and the write token for the control agent.
    let mut tokens = vec![];
    for (agent, access) in [("planner-reader", "read"), ("control-agent", "write")] {
        let (status, created) = post(
            &w,
            &w.owner,
            "/agent-tokens",
            &json!({"agent":agent,"access":access}).to_string(),
        )
        .await?;
        ensure!(
            status == 200 && created["access"] == access,
            "{status} {created}"
        );
        tokens.push(created["token"].as_str().unwrap().to_owned());
    }
    let (reader, writer) = (&tokens[0], &tokens[1]);

    let mcp_calls = [
        (
            "joppa_command",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"save_object","kind":"note","id":"from-mcp","revision":0,"parent":null,"data":{"title":"Saved through MCP","owner":"owner","body":"Input"},"reason":"Save"}}),
        ),
        (
            "joppa_batch",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"operations":[{"command":{"op":"claim_task","id":"task:claimable","revision":1}}]}),
        ),
        (
            "joppa_command",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"note","target":"note:comment-target","id":"control","text":"A control comment"}}),
        ),
        (
            "joppa_command",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"set_tree_schema","revision":tree["schema"]["revision"],"types":types,"reason":"Add a job type"}}),
        ),
    ];
    let junit = r#"<testsuites><testsuite name="tests/spec_http.rs"><testsuite name="spec"><testcase name="a_read_only_agent_token_cannot_write" /></testsuite></testsuite></testsuites>"#;
    let http_calls = [
        ("/blobs", format!("Read-only upload {}", uuid::Uuid::new_v4())),
        (
            "/agent-tokens",
            json!({"agent":"another-reader","access":"read"}).to_string(),
        ),
        (
            "/commands",
            json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"save_object","kind":"note","id":"from-http","revision":0,"parent":null,"data":{"title":"Saved through HTTP","owner":"owner","body":"Input"},"reason":"Save"}}).to_string(),
        ),
        (
            "/reports",
            json!({"stamp":{"commit":"abcdef1","order":1,"branch":"master","default_branch":true,"dirty":false,"full":true},"junit":junit}).to_string(),
        ),
    ];

    // Each read-token call is refused with the exact reply and stores nothing.
    let before = saved_rows(&w).await?;
    for (name, arguments) in &mcp_calls {
        let result = mcp_call(&w, reader, name, arguments.clone()).await?;
        ensure!(
            result == json!({"isError":true,"content":[{"type":"text","text":"read-only token"}]}),
            "{name} {arguments}: {result}"
        );
        ensure!(
            saved_rows(&w).await? == before,
            "{name} {arguments}: a refused tool stored data"
        );
    }
    for (path, body) in &http_calls {
        let (status, reply) = post(&w, reader, path, body).await?;
        ensure!(
            status == 403 && reply == json!({"error":"read-only token"}),
            "{path}: {status} {reply}"
        );
        ensure!(
            saved_rows(&w).await? == before,
            "{path}: a refused route stored data"
        );
    }

    // The same inputs succeed with a write grant, so each refusal comes from the grant.
    let mut last = before;
    for (name, arguments) in mcp_calls {
        let result = mcp_call(&w, writer, name, arguments.clone()).await?;
        ensure!(result["isError"] != true, "{name} {arguments}: {result}");
        let saved = saved_rows(&w).await?;
        ensure!(
            saved != last,
            "{name} {arguments}: the control stored nothing"
        );
        last = saved;
    }
    for (path, body) in http_calls {
        let token = if path == "/agent-tokens" {
            &w.owner
        } else {
            writer
        };
        let (status, reply) = post(&w, token, path, &body).await?;
        ensure!(status == 200, "{path}: {status} {reply}");
        let saved = saved_rows(&w).await?;
        ensure!(saved != last, "{path}: the control stored nothing");
        last = saved;
    }
    store::check_journal(&w.pool).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_token_without_access_writes_and_an_invalid_access_stores_nothing() -> Result<()> {
    let w = world().await?;
    let issue = |body: Value| {
        let w = &w;
        async move {
            let reply = w
                .http
                .post(format!("{}/api/workspaces/{}/agent-tokens", w.root, w.ws))
                .bearer_auth(&w.owner)
                .json(&body)
                .send()
                .await?;
            let code = reply.status().as_u16();
            let text = reply.text().await?;
            Ok::<_, anyhow::Error>((code, serde_json::from_str(&text).unwrap_or(json!(text))))
        }
    };
    let (code, created) = issue(json!({"agent":"bot"})).await?;
    ensure!(
        code == 200 && created["access"] == "write",
        "{code} {created}"
    );
    let token = created["token"].as_str().unwrap();
    ensure!(
        store::token_actor(&w.pool, token).await?.unwrap().access == store::Access::Write,
        "a token without access writes"
    );
    w.command_as(token, json!({"op":"save_object","kind":"note","id":"from-bot","revision":0,"parent":null,"data":{"title":"From bot","owner":"owner","body":"Input"},"reason":"Save"})).await?;
    let (code, created) = issue(json!({"agent":"bot","access":"write"})).await?;
    ensure!(
        code == 200 && created["access"] == "write",
        "{code} {created}"
    );

    let before = stored_counts(&w).await?;
    let (code, reply) = issue(json!({"agent":"stranger","access":"admin"})).await?;
    ensure!(code == 422, "{code} {reply}");
    let (code, reply) = issue(json!({"agent":"stranger","role":"read"})).await?;
    ensure!((400..500).contains(&code), "{code} {reply}");
    let refused = store::create_agent_token(&w.pool, &w.ws, "owner", "stranger", Some("admin"))
        .await
        .unwrap_err();
    ensure!(refused.is::<joppa::commands::ClientField>(), "{refused:#}");
    ensure!(
        stored_counts(&w).await? == before,
        "a refused issuance stored data"
    );
    let conn = w.pool.get().await?;
    let stranger: i64 = conn
        .query_one(
            "SELECT (SELECT count(*) FROM joppa.agent_tokens WHERE agent='stranger')
                  + (SELECT count(*) FROM joppa.state_entries WHERE section='members' AND key='stranger')",
            &[],
        )
        .await?
        .get(0);
    ensure!(stranger == 0, "the refused agent must not be stored");

    // A token row without access, as before the migration, reads as write and stays append-only.
    use sha2::{Digest, Sha256};
    let raw = format!("jpt_{}", uuid::Uuid::new_v4().simple());
    let id: i64 = conn
        .query_one(
            "INSERT INTO joppa.agent_tokens(token_hash,agent,workspace,created_by) VALUES($1,'bot',$2,'owner') RETURNING id",
            &[&format!("{:x}", Sha256::digest(raw.as_bytes())), &w.ws],
        )
        .await?
        .get(0);
    let grant = store::token_actor(&w.pool, &raw).await?.unwrap();
    ensure!(
        grant.agent == "bot" && grant.workspace == w.ws && grant.access == store::Access::Write,
        "{grant:?}"
    );
    let update = conn
        .execute(
            "UPDATE joppa.agent_tokens SET access='read' WHERE id=$1",
            &[&id],
        )
        .await;
    ensure!(update.is_err(), "agent tokens must stay append-only");
    Ok(())
}

// [ac:criterion-checkable-ac1]
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_bot_criterion_title_over_64_characters_is_refused() -> Result<()> {
    let w = world().await?;
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/agent-tokens", w.root, w.ws))
        .bearer_auth(&w.owner)
        .json(&json!({"agent":"bot","access":"write"}))
        .send()
        .await?;
    ensure!(reply.status().is_success(), "{}", reply.text().await?);
    let created: Value = reply.json().await?;
    ensure!(created["access"] == "write", "{created}");
    let token = created["token"].as_str().unwrap().to_owned();
    let grant = store::token_actor(&w.pool, &token).await?.unwrap();
    ensure!(
        grant.agent == "bot" && grant.workspace == w.ws && grant.access == store::Access::Write,
        "{grant:?}"
    );
    let read = |address: &'static str| {
        let (w, token) = (&w, &token);
        async move {
            let reply = w
                .http
                .get(format!("{}/api/workspaces/{}/objects", w.root, w.ws))
                .bearer_auth(token)
                .query(&[("address", address)])
                .send()
                .await?;
            let status = reply.status().as_u16();
            Ok::<_, anyhow::Error>((status, reply.json::<Value>().await?))
        }
    };
    let criterion = |id: &str, title: String| json!({"op":"save_object","kind":"ac","id":id,"revision":0,"parent":"requirement:key","data":{"title":title,"owner":"owner","description":"Given: a requirement.\nWhen: bot saves a criterion.\nThen: the title fits.\nEvidence: the saved criterion."},"reason":"Check the title length"});

    let (status, saved) = text_rule_request(
        &w,
        &token,
        "fits-title",
        criterion("fits", "a".repeat(64)),
        "http",
    )
    .await?;
    ensure!(status == 200, "{status}: {saved}");
    let (status, fits) = read("ac:fits").await?;
    ensure!(
        status == 200 && fits["document"]["data"]["title"] == "a".repeat(64),
        "{status}: {fits}"
    );

    let conn = w.pool.get().await?;
    let counts = || async {
        let row = conn
            .query_one(
                "SELECT (SELECT count(*) FROM joppa.events WHERE workspace=$1),
                        (SELECT count(*) FROM joppa.object_revisions WHERE workspace=$1),
                        (SELECT count(*) FROM joppa.command_receipts WHERE workspace=$1)",
                &[&w.ws],
            )
            .await?;
        Ok::<_, anyhow::Error>((0..3).map(|i| row.get(i)).collect::<Vec<i64>>())
    };
    let before = counts().await?;
    let (status, denied) = text_rule_request(
        &w,
        &token,
        "long-title",
        criterion("long", "a".repeat(65)),
        "http",
    )
    .await?;
    ensure!(
        !(200..300).contains(&status)
            && denied["error"]
                == "an ac title is at most 64 characters; put the details in the text",
        "{status}: {denied}"
    );
    ensure!(
        counts().await? == before,
        "a refused title stored an event, revision or receipt"
    );
    let (status, missing) = read("ac:long").await?;
    ensure!(
        !(200..300).contains(&status) && missing == json!({"error":"unknown object"}),
        "the refused criterion is readable: {status}: {missing}"
    );
    Ok(())
}

/// One REST command; returns whether it succeeded and the reply body.
async fn send_as(w: &World, token: &str, command: &Value) -> Result<(bool, Value)> {
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(token)
        .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": command}))
        .send()
        .await?;
    Ok((reply.status().is_success(), reply.json().await?))
}

/// The JSON value of one successful MCP tools/call.
async fn mcp_value(w: &World, name: &str, arguments: Value) -> Result<Value> {
    let result = mcp_call(w, &w.bot, name, arguments).await?;
    ensure!(result["isError"] != true, "{result}");
    Ok(serde_json::from_str(
        result["content"][0]["text"].as_str().unwrap_or("null"),
    )?)
}

/// Bot search over MCP; fails on HTTP, JSON-RPC or tool errors. Returns addresses and the reply.
async fn bot_search(w: &World, query: &str) -> Result<(Vec<String>, Value)> {
    let result = mcp_call(
        w,
        &w.bot,
        "joppa_search",
        json!({"workspace":w.ws,"query":query}),
    )
    .await?;
    search_addresses(&result)
}

/// Addresses of a joppa_search tool result; a tool error or a reply without a result array fails.
fn search_addresses(result: &Value) -> Result<(Vec<String>, Value)> {
    ensure!(result["isError"] != true, "tool error: {result}");
    let text = result["content"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no text: {result}"))?;
    let found: Value = serde_json::from_str(text).map_err(|e| anyhow::anyhow!("{e}: {result}"))?;
    let rows = found["items"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("no result array: {result}"))?;
    let addresses = rows
        .iter()
        .map(|r| match (r["kind"].as_str(), r["id"].as_str()) {
            (Some(kind), Some(id)) => Ok(format!("{kind}:{id}")),
            _ => anyhow::bail!("result without an address: {result}"),
        })
        .collect::<Result<_>>()?;
    Ok((addresses, found))
}

#[test]
fn search_addresses_rejects_a_tool_error_with_the_reply() {
    let reply = json!({"isError":true,"content":[{"type":"text","text":"search failed"}]});
    let err = search_addresses(&reply).unwrap_err().to_string();
    assert!(err.contains("search failed"), "{err}");
}

#[test]
fn search_addresses_rejects_a_reply_without_a_result_array() {
    let reply = json!({"content":[{"type":"text","text":"{\"mode\":\"full_text\"}"}]});
    let err = search_addresses(&reply).unwrap_err().to_string();
    assert!(
        err.contains("no result array") && err.contains("full_text"),
        "{err}"
    );
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn help_operations_equal_the_command_op_enum() -> Result<()> {
    use std::collections::BTreeSet;
    let w = world().await?;
    let reply: Value = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let tools = reply["result"]["tools"].as_array().unwrap();
    let schema = |name: &str| {
        tools
            .iter()
            .find(|t| t["name"] == name)
            .map(|t| t["inputSchema"].clone())
            .unwrap_or_default()
    };
    let names = |command: &Value| -> BTreeSet<String> {
        command["properties"]["op"]["enum"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    };
    let command = names(&schema("joppa_command")["properties"]["command"]);
    let batch =
        names(&schema("joppa_batch")["properties"]["operations"]["items"]["properties"]["command"]);
    ensure!(
        !command.is_empty() && batch == command,
        "{command:?} / {batch:?}"
    );
    for name in [
        "heartbeat_task",
        "workspace",
        "enroll_member",
        "migrate_persistent_claims",
        "migrate_notes",
        "migrate_tasks",
        "migrate_schema",
        "migrate_documents",
        "migrate_task_claims",
    ] {
        ensure!(!command.contains(name), "{name} is in the op enum");
    }
    for arguments in [json!({}), json!({"workspace":w.ws})] {
        let help = mcp_value(&w, "joppa_help", arguments.clone()).await?;
        let operations = help["operations"].as_object().unwrap();
        let keys: BTreeSet<String> = operations.keys().cloned().collect();
        ensure!(keys == command, "help {keys:?} / enum {command:?}");
        for (name, entry) in operations {
            let callers = entry["callers"].as_array().unwrap();
            ensure!(
                !callers.is_empty()
                    && callers.iter().all(|c| {
                        ["member", "agent", "holder", "workspace owner"]
                            .contains(&c.as_str().unwrap_or(""))
                    })
                    && entry["required"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(Value::is_string),
                "{name}: {entry}"
            );
        }
        ensure!(
            help["search"].is_string()
                && help["writing"].is_string()
                && help["objects"].is_string()
                && help.get("tree").is_some() == arguments.get("workspace").is_some(),
            "other help fields stay"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn unknown_and_internal_operations_are_refused_without_writes() -> Result<()> {
    let w = world().await?;
    let before = saved_rows(&w).await?;
    let valid = json!({"op":"member","id":"should-not-exist","role":"agent"});
    for command in [
        json!({"op":"made_up_operation","id":"x"}),
        json!({"op":"migrate_notes"}),
        json!({"op":"enroll_member","id":"intruder"}),
    ] {
        let (ok, http) = send_as(&w, &w.owner, &command).await?;
        ensure!(!ok && http["error"] == "unknown operation", "{http}");
        let single = mcp_call(
            &w,
            &w.owner,
            "joppa_command",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
        )
        .await?;
        ensure!(
            single["isError"] == true && single["content"][0]["text"] == "unknown operation",
            "{single}"
        );
        let batch = mcp_call(
            &w,
            &w.owner,
            "joppa_batch",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"operations":[{"command":valid},{"command":command}]}),
        )
        .await?;
        ensure!(
            batch["isError"] == true
                && batch["content"][0]["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("unknown operation")),
            "{batch}"
        );
    }
    ensure!(
        saved_rows(&w).await? == before,
        "a refused operation changed state"
    );
    let members = store::read(&w.pool, &w.ws, "owner", &store::Scope::full()).await?;
    ensure!(
        !members.to_string().contains("should-not-exist"),
        "the batch applied its valid operation"
    );
    Ok(())
}

/// Each caller that the help entry does not name is refused and changes nothing.
/// Then a named caller sends the same command and succeeds.
async fn check_callers(
    w: &World,
    help: &Value,
    holder: Option<&str>,
    command: Value,
    allowed: &str,
    refusal: &str,
) -> Result<String> {
    let name = command["op"].as_str().unwrap().to_owned();
    let callers: Vec<&str> = help["operations"][&name]["callers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let named = |token: &str| {
        let mut categories = vec!["member"];
        if token == w.bot {
            categories.push("agent");
        }
        if token == w.owner {
            categories.push("workspace owner");
        }
        if Some(token) == holder {
            categories.push("holder");
        }
        categories.iter().any(|c| callers.contains(c))
    };
    ensure!(named(allowed), "{name}: the allowed caller is not named");
    let before = saved_rows(w).await?;
    let mut refused = 0;
    for token in [&w.owner, &w.alex, &w.maria, &w.bot] {
        if named(token) {
            continue;
        }
        refused += 1;
        let (ok, reply) = send_as(w, token, &command).await?;
        ensure!(
            !ok && reply["error"].as_str().is_some_and(|e| e.contains(refusal)),
            "{name}: {reply}"
        );
    }
    ensure!(refused > 0, "{name} has no unnamed caller");
    ensure!(
        saved_rows(w).await? == before,
        "{name}: a refusal changed state"
    );
    let (ok, reply) = send_as(w, allowed, &command).await?;
    ensure!(ok, "{name}: {reply}");
    Ok(name)
}

// signal-becomes-spec-ac2
// A member may archive an Inbox object, so the archive refusal uses a capability.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn restricted_operations_refuse_callers_that_help_does_not_name() -> Result<()> {
    let w = world().await?;
    let help = mcp_value(&w, "joppa_help", json!({})).await?;
    let revision = async |address: &str| -> Result<Value> {
        Ok(w.get(&format!("address={address}")).await?["current_revision"].clone())
    };
    let owner_only = "workspace owner required";
    let agent_or_owner = "workspace owner or agent required";
    let mut covered = vec![];
    let schema = store::tree_schema(&w.pool, &w.ws, "owner", None).await?;
    let mut types = schema["schema"]["types"].clone();
    types["job"] = json!({"title":"Jobs","role":"task","root":true,"parents":["*"]});
    covered.push(check_callers(&w, &help, None, json!({"op":"set_tree_schema","revision":schema["schema"]["revision"],"types":types,"reason":"Use configured work"}), &w.bot, agent_or_owner).await?);
    for id in ["a", "b", "c"] {
        w.command(json!({"op":"save_object","kind":"job","id":id,"revision":0,"parent":null,"data":{"title":"Implement","owner":"alex","contract":"Implement and check the contract","purpose":"implementation"},"reason":"Create work"})).await?;
    }
    w.note("secret", 0, "Secret note", "Token s3cr3t", json!([]))
        .await?;
    covered.push(check_callers(&w, &help, None, json!({"op":"archive_object","target":"capability:empty","revision":revision("capability:empty").await?,"reason":"Hide the capability"}), &w.bot, agent_or_owner).await?);
    w.command(json!({"op":"archive_object","target":"note:secret","revision":revision("note:secret").await?,"reason":"Hide the note"})).await?;
    covered.push(check_callers(&w, &help, None, json!({"op":"restore_object","target":"note:secret","revision":revision("note:secret").await?,"reason":"Show the note"}), &w.bot, agent_or_owner).await?);
    covered.push(check_callers(&w, &help, None, json!({"op":"redact_object","target":"note:secret","fragment":"s3cr3t","reason":"Remove the secret"}), &w.owner, owner_only).await?);
    covered.push(
        check_callers(
            &w,
            &help,
            None,
            json!({"op":"rename_workspace","title":"Spec over HTTP again","reason":"Rename"}),
            &w.owner,
            "workspace owner renames the workspace",
        )
        .await?,
    );
    covered.push(check_callers(&w, &help, None, json!({"op":"reorder_tree","kind":"job","parent":null,"expected":["a","b","c"],"ids":["c","b","a"]}), &w.owner, "workspace owner reorders the tree").await?);
    covered.push(
        check_callers(
            &w,
            &help,
            None,
            json!({"op":"member","id":"newcomer","role":"agent"}),
            &w.owner,
            owner_only,
        )
        .await?,
    );
    w.command(json!({"op":"domain","id":"spare","title":"Spare","owner":"owner"}))
        .await?;
    covered.push(
        check_callers(
            &w,
            &help,
            None,
            json!({"op":"set_domain_archived","id":"spare","archived":true,"reason":"Unused"}),
            &w.owner,
            "workspace owner archives domains",
        )
        .await?,
    );
    covered.push(
        check_callers(
            &w,
            &help,
            None,
            json!({"op":"confirm_tree"}),
            &w.owner,
            "workspace owner confirms the tree",
        )
        .await?,
    );
    covered.push(check_callers(&w, &help, None, json!({"op":"edit_domain","id":"security","title":"Security rules","owner":"owner","reason":"Clarify"}), &w.owner, "only the workspace owner edits confirmed domains").await?);
    covered.push(check_callers(&w, &help, None, json!({"op":"edit_capability","id":"restrict","title":"Restrict all access","reason":"Clarify"}), &w.owner, "only the workspace owner edits confirmed capabilities").await?);
    // alex owns each job. The Task owner is neither the holder nor the workspace owner.
    let bot = Some(w.bot.as_str());
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"job:a","revision":revision("job:a").await?}),
    )
    .await?;
    covered.push(check_callers(&w, &help, bot, json!({"op":"block_task","id":"job:a","revision":revision("job:a").await?,"reason":"The test fails","evidence":"test a failed at commit c1"}), &w.bot, "Task holder required").await?);
    covered.push(check_callers(&w, &help, None, json!({"op":"unblock_task","id":"job:a","revision":revision("job:a").await?,"reason":"The failure was outside the Task","evidence":"Run: r1"}), &w.owner, "planning holder or workspace owner required").await?);
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"job:b","revision":revision("job:b").await?}),
    )
    .await?;
    check_callers(&w, &help, bot, json!({"op":"block_task","id":"job:b","revision":revision("job:b").await?,"reason":"The test fails","evidence":"test b failed at commit c1"}), &w.owner, "Task holder required").await?;
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"job:c","revision":revision("job:c").await?}),
    )
    .await?;
    covered.push(check_callers(&w, &help, bot, json!({"op":"release_task","id":"job:c","revision":revision("job:c").await?,"reason":"Return the work"}), &w.bot, owner_only).await?);
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"job:c","revision":revision("job:c").await?}),
    )
    .await?;
    covered.push(check_callers(&w, &help, bot, json!({"op":"complete_task","id":"job:c","revision":revision("job:c").await?,"evidence":"Implemented and checked"}), &w.bot, "Task is not claimed unfinished work").await?);
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command_as(&w.bot, json!({"op":"claim_planning","ac":"ac:key-rights","revision":revision("ac:key-rights").await?})).await?;
    covered.push(
        check_callers(
            &w,
            &help,
            bot,
            json!({"op":"release_planning","ac":"ac:key-rights","reason":"Plan later"}),
            &w.bot,
            owner_only,
        )
        .await?,
    );
    w.command_as(&w.bot, json!({"op":"claim_planning","ac":"ac:key-rights","revision":revision("ac:key-rights").await?})).await?;
    w.command_as(&w.bot, json!({"op":"save_object","kind":"task","id":"planned","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the key rights","owner":"owner","contract":"Implement the key rights and record the result","purpose":"implementation","projects":[]},"reason":"Plan the work"})).await?;
    covered.push(
        check_callers(
            &w,
            &help,
            bot,
            json!({"op":"finish_planning","ac":"ac:key-rights","reason":"The plan is complete"}),
            &w.bot,
            "planning holder required",
        )
        .await?,
    );
    covered.push(
        check_callers(
            &w,
            &help,
            None,
            json!({"op":"remove_member","id":"newcomer","reason":"Remove access"}),
            &w.owner,
            owner_only,
        )
        .await?,
    );
    // The deletion comes last: after it, the workspace accepts no command.
    covered.push(
        check_callers(
            &w,
            &help,
            None,
            json!({"op":"delete_workspace","reason":"Close the workspace"}),
            &w.owner,
            "workspace owner deletes the workspace",
        )
        .await?,
    );
    let mut restricted: Vec<String> = help["operations"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, entry)| {
            !entry["callers"]
                .as_array()
                .unwrap()
                .contains(&json!("member"))
        })
        .map(|(name, _)| name.clone())
        .collect();
    restricted.sort();
    covered.sort();
    ensure!(
        covered == restricted,
        "covered {covered:?} / restricted {restricted:?}"
    );
    Ok(())
}

#[path = "common/operation_samples.rs"]
mod operation_samples;

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn help_required_fields_pass_the_live_field_checks() -> Result<()> {
    let w = world().await?;
    let help = mcp_value(&w, "joppa_help", json!({})).await?;
    let required = |name: &str| -> Vec<String> {
        help["operations"][name]["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|f| f.as_str().map(String::from))
            .collect()
    };
    // A ready criterion is the prerequisite state of a legacy Task.
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let task = json!({"op":"task","req":"key","revision":1,"title":"Build key rights","acs":["key-rights"],"projects":[],"purpose":"implementation","contract":"Implement and provide evidence"});
    let mut fields: Vec<String> = task
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| *k != "op")
        .cloned()
        .collect();
    fields.sort();
    let mut expected = required("task");
    expected.sort();
    ensure!(fields == expected, "task help fields {expected:?}");
    for field in &fields {
        let mut missing = task.clone();
        missing.as_object_mut().unwrap().remove(field);
        let (ok, reply) = send_as(&w, &w.owner, &missing).await?;
        ensure!(
            !ok && reply["error"]
                .as_str()
                .is_some_and(|e| e.contains("missing field")),
            "task without {field}: {reply}"
        );
    }
    let (ok, reply) = send_as(&w, &w.owner, &task).await?;
    ensure!(ok, "task from help fields: {reply}");
    // Every help entry: the complete call passes field validation; each removal fails it.
    for name in help["operations"].as_object().unwrap().keys() {
        let fields = required(name);
        let fields: Vec<&str> = fields.iter().map(String::as_str).collect();
        let command = operation_samples::call(name, &fields);
        let (_, reply) = send_as(&w, &w.owner, &command).await?;
        ensure!(
            !reply["error"]
                .as_str()
                .is_some_and(|e| e.contains("missing field")),
            "{name} from help fields: {reply}"
        );
        for field in &fields {
            let mut missing = command.clone();
            missing.as_object_mut().unwrap().remove(*field);
            let (ok, reply) = send_as(&w, &w.owner, &missing).await?;
            ensure!(
                !ok && reply["error"]
                    .as_str()
                    .is_some_and(|e| e.contains("missing field")),
                "{name} without {field}: {reply}"
            );
        }
    }
    Ok(())
}

/// Create one ready implementation Task under an AC of a REQ.
async fn ready_task(w: &World, id: &str, ac: &str) -> Result<String> {
    w.command(ready_task_command(id, ac)).await?;
    Ok(format!("task:{id}"))
}

fn ready_task_command(id: &str, ac: &str) -> Value {
    json!({"op":"save_object","kind":"task","id":id,"revision":0,"parent":format!("ac:{ac}"),"data":{"title":"Implement the specified work","owner":"owner","contract":"Implement the criterion and record the result","purpose":"implementation","projects":[]},"reason":"Plan the criterion"})
}

async fn journal_position(w: &World) -> Result<i64> {
    let conn = w.pool.get().await?;
    let row = conn
        .query_one(
            "SELECT COALESCE(max(position),0) FROM joppa.events WHERE workspace=$1",
            &[&w.ws],
        )
        .await?;
    Ok(row.get(0))
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_identical_save_adds_no_revision_or_journal_event() -> Result<()> {
    // edit-makes-revision-ac3
    let w = world().await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let t = ready_task(&w, "t", "key-rights").await?;
    let save = |revision: u32, title: &str| json!({"op":"save_object","kind":"task","id":"t","revision":revision,"parent":"ac:key-rights","data":{"title":title,"owner":"owner","contract":"Implement the changed criterion and record the result","purpose":"implementation","projects":[]},"reason":"Save the work contract"});
    let title = "Implement the specified work";
    w.command(save(1, title)).await?;
    let read = async |address: &str| w.get(&format!("address={address}")).await;
    let revisions = async || w.get("address=task:t&history=true").await;
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":t,"revision":read(&t).await?["current_revision"]}),
    )
    .await?;
    w.command_as(&w.bot, json!({"op":"complete_task","id":t,"revision":read(&t).await?["current_revision"],"evidence":"Implemented the contract"})).await?;
    let done = read(&t).await?;
    ensure!(
        done["current_revision"] == 4
            && done["process"]["status"] == "done"
            && done["process"]["assignee"] == "bot",
        "{done}"
    );
    w.command(json!({"op":"save_object","kind":"task","id":"u","revision":0,"parent":"ac:key-rights","data":{"title":"Use the specified work","owner":"owner","contract":"Use the completed work and record the result","purpose":"implementation","projects":[]},"links":[{"target":t,"relation":"depends_on","revision":4,"blocking":true}],"reason":"Depend on the completed work"})).await?;
    let position = journal_position(&w).await?;

    let http = w.command_as(&w.bot, save(4, title)).await?;
    ensure!(
        http["object"] == json!({"address":t,"revision":4}),
        "{http}"
    );
    let mcp = mcp_value(&w, "joppa_command", json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":save(4, title)})).await?;
    ensure!(mcp["object"] == json!({"address":t,"revision":4}), "{mcp}");
    let task = revisions().await?;
    ensure!(
        task["current_revision"] == 4 && task["revision_count"] == 4,
        "{task}"
    );
    let u = read("task:u").await?;
    ensure!(
        u["review_required"] == false
            && u["document"]["links"][0]["revision"] == task["current_revision"],
        "{u}"
    );
    let available = mcp_value(
        &w,
        "joppa_tasks",
        json!({"workspace":w.ws,"view":"available"}),
    )
    .await?;
    ensure!(
        available["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["address"] == "task:u" && row["claimable"] == true),
        "{available}"
    );
    ensure!(journal_position(&w).await? == position);

    for (token, command, error) in [
        (
            &w.bot,
            save(4, "Implement other work"),
            "completed or retired Task cannot be amended",
        ),
        (
            &w.bot,
            save(3, title),
            "stale object revision; read the object again",
        ),
        (
            &w.maria,
            save(4, title),
            "Task owner, assignee or unclaimed creator required",
        ),
    ] {
        let reply = w
            .http
            .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
            .bearer_auth(token)
            .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":command}))
            .send()
            .await?;
        ensure!(!reply.status().is_success(), "{command}");
        let reply: Value = reply.json().await?;
        ensure!(reply["error"] == error, "{command}: {reply}");
    }
    let task = revisions().await?;
    ensure!(
        task["current_revision"] == 4 && task["revision_count"] == 4,
        "{task}"
    );
    ensure!(journal_position(&w).await? == position);
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_claims_and_completes_all_tasks_of_a_req_in_one_batch() -> Result<()> {
    // one-task-per-agent-ac3
    let w = world().await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let mut tasks = vec![];
    for id in ["t1", "t2", "t3"] {
        tasks.push(ready_task(&w, id, "key-rights").await?);
    }
    let read = async |address: &str| w.get(&format!("address={address}")).await;
    let claims: Vec<Value> = tasks
        .iter()
        .map(|id| json!({"command":{"op":"claim_task","id":id,"revision":1}}))
        .collect();
    mcp_value(
        &w,
        "joppa_batch",
        json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"operations":claims}),
    )
    .await?;
    let mut completions = vec![];
    for id in &tasks {
        let task = read(id).await?;
        ensure!(
            task["process"]["assignee"] == "bot" && task["process"]["status"] == "in_progress",
            "{task}"
        );
        completions.push(json!({"command":{"op":"complete_task","id":id,"revision":task["current_revision"],"evidence":"Implemented the criterion and ran its checks"}}));
    }
    mcp_value(&w, "joppa_batch", json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"operations":completions})).await?;
    let mut done = vec![];
    for id in &tasks {
        let task = read(id).await?;
        ensure!(
            task["process"]["assignee"] == "bot" && task["process"]["status"] == "done",
            "{task}"
        );
        done.push(task);
    }
    w.pool
        .get()
        .await?
        .execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&w.ws])
        .await?;
    store::migrate(&w.pool).await?;
    for (id, before) in tasks.iter().zip(&done) {
        let after = read(id).await?;
        ensure!(
            after["process"] == before["process"],
            "journal replay changed {id}: {before} / {after}"
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_batch_claim_with_a_held_task_changes_nothing() -> Result<()> {
    // one-task-per-agent-ac4
    let w = world().await?;
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    let mut tasks = vec![];
    for id in ["t1", "t2", "t3"] {
        tasks.push(ready_task(&w, id, "key-rights").await?);
    }
    let other = store::create_agent_token(&w.pool, &w.ws, "owner", "other", None).await?;
    let other = other["token"].as_str().unwrap();
    w.command_as(other, json!({"op":"claim_task","id":tasks[2],"revision":1}))
        .await?;
    let read = async |address: &str| w.get(&format!("address={address}")).await;
    let mut claims = vec![];
    for id in &tasks {
        let task = read(id).await?;
        claims.push(
            json!({"command":{"op":"claim_task","id":id,"revision":task["current_revision"]}}),
        );
    }
    let position = journal_position(&w).await?;
    let before = saved_rows(&w).await?;

    let batch = mcp_call(
        &w,
        &w.bot,
        "joppa_batch",
        json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"operations":claims}),
    )
    .await?;
    ensure!(batch["isError"] == true, "{batch}");
    for id in &tasks[..2] {
        let task = read(id).await?;
        ensure!(task["process"]["assignee"].is_null(), "{task}");
    }
    let held = read(&tasks[2]).await?;
    ensure!(held["process"]["assignee"] == "other", "{held}");
    ensure!(journal_position(&w).await? == position);
    ensure!(
        saved_rows(&w).await? == before,
        "a refused batch changed state"
    );

    let t1 = read(&tasks[0]).await?;
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":tasks[0],"revision":t1["current_revision"]}),
    )
    .await?;
    let t1 = read(&tasks[0]).await?;
    ensure!(t1["process"]["assignee"] == "bot", "{t1}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_holding_a_task_cannot_claim_a_task_of_another_req() -> Result<()> {
    let w = world().await?;
    w.command(json!({"op":"revise","req":"other","expected_revision":0,"capability":"restrict","title":"Other rights","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"other-rights","text":"Only write after review"}]})).await?;
    for (req, ac) in [("key", "key-rights"), ("other", "other-rights")] {
        w.command_as(&w.bot, json!({"op":"assess","req":req,"revision":1,"ac":ac,"assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    }
    let held = ready_task(&w, "a1", "key-rights").await?;
    let same = ready_task(&w, "a2", "key-rights").await?;
    let other = ready_task(&w, "b1", "other-rights").await?;
    w.command_as(&w.bot, json!({"op":"claim_task","id":held,"revision":1}))
        .await?;
    let queue = async |view: &str| {
        mcp_value(&w, "joppa_tasks", json!({"workspace":w.ws,"view":view})).await
    };
    let available = queue("available").await?;
    ensure!(
        available["tasks"].as_array().unwrap().len() == 1
            && available["tasks"][0]["address"] == same
            && available["tasks"][0]["claimable"] == true,
        "{available}"
    );
    let waiting = queue("waiting").await?;
    let reason = format!("agent already holds task {held}");
    ensure!(
        waiting["tasks"].as_array().unwrap().len() == 1
            && waiting["tasks"][0]["address"] == other
            && waiting["tasks"][0]["claimable"] == false
            && waiting["tasks"][0]["waiting_reason"] == reason,
        "{waiting}"
    );
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.bot)
        .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"claim_task","id":other,"revision":1}}))
        .send()
        .await?;
    ensure!(
        reply.status() == reqwest::StatusCode::CONFLICT,
        "{}",
        reply.status()
    );
    let reply: Value = reply.json().await?;
    ensure!(reply["error"] == reason, "{reply}");
    let refused = w.get(&format!("address={other}")).await?;
    ensure!(refused["process"]["assignee"].is_null(), "{refused}");
    let next = w
        .command_as(&w.bot, json!({"op":"claim_next_task"}))
        .await?;
    ensure!(next["command"]["id"] == same, "{next}");
    Ok(())
}

/// REQ key holds task:t1; REQ other holds task:t2 under other-rights and a second AC other-audit.
async fn tasks_under_two_requirements(w: &World) -> Result<()> {
    w.command(json!({"op":"revise","req":"other","expected_revision":0,"capability":"restrict","title":"Other rights","body":"Scope","owner":"owner","consumers":["customer"],"reason":"Agreed","acs":[{"id":"other-rights","text":"Only write after review"},{"id":"other-audit","text":"Record each write"}]})).await?;
    for (req, ac) in [
        ("key", "key-rights"),
        ("other", "other-rights"),
        ("other", "other-audit"),
    ] {
        w.command_as(&w.bot, json!({"op":"assess","req":req,"revision":1,"ac":ac,"assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    }
    ready_task(w, "t1", "key-rights").await?;
    ready_task(w, "t2", "other-rights").await?;
    Ok(())
}

/// A Task save with one blocking link to `target` at revision 1.
fn linked_task_command(id: &str, revision: u32, ac: &str, target: &str) -> Value {
    let mut command = ready_task_command(id, ac);
    command["revision"] = json!(revision);
    command["links"] =
        json!([{"target":target,"relation":"depends_on","revision":1,"blocking":true}]);
    command
}

// task-links-stay-in-req-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_link_to_a_task_of_another_requirement_is_refused() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    let error = "a Task can depend only on a Task under the same requirement";
    let t2 = w.get("address=task:t2").await?;
    ensure!(t2["document"]["links"] == json!([]), "{t2}");
    let saved = saved_rows(&w).await?;
    for command in [
        linked_task_command("t3", 0, "other-rights", "task:t1"),
        linked_task_command("t2", 1, "other-rights", "task:t1"),
    ] {
        let (ok, http) = send_as(&w, &w.bot, &command).await?;
        ensure!(!ok && http["error"] == error, "HTTP {command}: {http}");
        let mcp = mcp_call(
            &w,
            &w.bot,
            "joppa_command",
            json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
        )
        .await?;
        ensure!(
            mcp["isError"] == true && mcp["content"][0]["text"] == error,
            "MCP {command}: {mcp}"
        );
    }
    ensure!(
        saved_rows(&w).await? == saved,
        "a refused Task link stored an object, revision or journal row"
    );
    let after = w.get("address=task:t2").await?;
    ensure!(
        after["document"]["links"] == json!([])
            && after["current_revision"] == t2["current_revision"],
        "{after}"
    );
    let t3 = w
        .http
        .get(format!(
            "{}/api/workspaces/{}/objects?address=task:t3",
            w.root, w.ws
        ))
        .bearer_auth(&w.bot)
        .send()
        .await?;
    let status = t3.status();
    let t3: Value = t3.json().await?;
    ensure!(
        !status.is_success() && t3 == json!({"error":"unknown object"}),
        "{status}: {t3}"
    );
    Ok(())
}

// task-links-stay-in-req-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_link_across_criteria_of_one_requirement_is_saved() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    w.command_as(
        &w.bot,
        linked_task_command("t4", 0, "other-audit", "task:t2"),
    )
    .await?;
    let t4 = w.get_as(&w.bot, "address=task:t4").await?;
    ensure!(
        t4["document"]["links"]
            == json!([{"target":"task:t2","relation":"depends_on","revision":1,"blocking":true}]),
        "{t4}"
    );
    Ok(())
}

// task-links-stay-in-req-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_task_link_added_across_criteria_of_one_requirement_waits_for_its_dependency()
-> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    // Bot creates task:t4 so that bot, as its unclaimed creator, may add the link.
    w.command_as(&w.bot, ready_task_command("t4", "other-audit"))
        .await?;
    let t4 = "task:t4";
    let (ok, saved) = send_as(
        &w,
        &w.bot,
        &linked_task_command("t4", 1, "other-audit", "task:t2"),
    )
    .await?;
    ensure!(ok, "{saved}");
    let read = w.get_as(&w.bot, &format!("address={t4}")).await?;
    let links = &read["document"]["links"];
    ensure!(
        links[0]["target"] == "task:t2" && links[0]["blocking"] == true,
        "{read}"
    );
    let (ok, early) = send_as(
        &w,
        &w.bot,
        &json!({"op":"claim_task","id":t4,"revision":read["current_revision"]}),
    )
    .await?;
    ensure!(
        !ok && early["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("Task has an unmet dependency")),
        "{early}"
    );
    let waiting = w.get(&format!("address={t4}")).await?;
    ensure!(waiting["process"]["assignee"].is_null(), "{waiting}");
    let current = async |address: &str| -> Result<Value> {
        Ok(w.get(&format!("address={address}")).await?["current_revision"].clone())
    };
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"task:t2","revision":current("task:t2").await?}),
    )
    .await?;
    w.command_as(&w.bot, json!({"op":"complete_task","id":"task:t2","revision":current("task:t2").await?,"evidence":"Implemented the contract"})).await?;
    let (ok, claimed) = send_as(
        &w,
        &w.bot,
        &json!({"op":"claim_task","id":t4,"revision":current(t4).await?}),
    )
    .await?;
    ensure!(ok, "{claimed}");
    Ok(())
}

// task-links-stay-in-req-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_journal_with_a_cross_requirement_task_link_still_replays() -> Result<()> {
    let w = world().await?;
    tasks_under_two_requirements(&w).await?;
    // Seed the historical link straight into the journal; live saves now refuse it.
    let command = linked_task_command("t2", 1, "other-rights", "task:t1");
    let position = journal_position(&w).await? + 1;
    let conn = w.pool.get().await?;
    conn.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,'owner',$3,$4::text::jsonb)",
        &[&w.ws, &position, &uuid::Uuid::new_v4().to_string(), &command.to_string()],
    )
    .await?;
    conn.execute("DELETE FROM joppa.board_cache WHERE workspace=$1", &[&w.ws])
        .await?;
    store::backfill_board(&w.pool).await?;
    let t2 = w.get("address=task:t2").await?;
    ensure!(
        t2["current_revision"] == 2
            && t2["document"]["links"]
                == json!([{"target":"task:t1","relation":"depends_on","revision":1,"blocking":true}]),
        "{t2}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_holding_a_task_claims_a_task_in_another_workspace() -> Result<()> {
    // one-task-per-agent-ac2
    let w = world().await?;
    let assess = json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}});
    w.command_as(&w.bot, assess.clone()).await?;
    let t1 = ready_task(&w, "t1", "key-rights").await?;
    w.command_as(&w.bot, json!({"op":"claim_task","id":t1,"revision":1}))
        .await?;
    let w2 = format!("http-spec-{}", uuid::Uuid::new_v4().simple());
    seed(
        &w.pool,
        &w2,
        vec![
            ("bot", assess),
            ("owner", ready_task_command("t3", "key-rights")),
        ],
    )
    .await?;
    let t3 = "task:t3";

    let reply = w
        .http
        .post(format!("{}/api/workspaces/{w2}/commands", w.root))
        .bearer_auth(&w.bot)
        .json(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"claim_task","id":t3,"revision":1}}))
        .send()
        .await?;
    let status = reply.status();
    let body = reply.text().await?;
    let read_t3 = w
        .http
        .get(format!(
            "{}/api/workspaces/{w2}/objects?address={t3}",
            w.root
        ))
        .bearer_auth(&w.owner)
        .send()
        .await?;
    ensure!(read_t3.status().is_success(), "{}", read_t3.text().await?);
    let t3: Value = read_t3.json().await?;
    let t1 = w.get(&format!("address={t1}")).await?;
    let states = format!("reply {status} {body}; T3 {t3}; T1 {t1}");
    ensure!(status == reqwest::StatusCode::OK, "{states}");
    ensure!(
        t3["process"]["assignee"] == "bot" && t3["process"]["status"] == "in_progress",
        "{states}"
    );
    ensure!(t1["process"]["assignee"] == "bot", "{states}");
    Ok(())
}

/// `token` sends `command` over HTTP and then MCP. Each transport must refuse with `expected`
/// and keep the Task revision, process.blocked, the journal position and all saved rows.
async fn refuse_task_action(
    w: &World,
    case: &str,
    token: &str,
    command: Value,
    expected: &str,
) -> Result<()> {
    let address = command["id"].as_str().unwrap();
    for transport in ["HTTP", "MCP"] {
        let task = w.get(&format!("address={address}")).await?;
        let before = saved_rows(w).await?;
        let request_id = uuid::Uuid::new_v4().to_string();
        let (refused, error, reply) = if transport == "MCP" {
            let result = mcp_call(
                w,
                token,
                "joppa_command",
                json!({"workspace":w.ws,"request_id":request_id,"command":command}),
            )
            .await?;
            (
                result["isError"] == true,
                result["content"][0]["text"].clone(),
                result,
            )
        } else {
            let response = w
                .http
                .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
                .bearer_auth(token)
                .json(&json!({"request_id":request_id,"command":command}))
                .send()
                .await?;
            let refused = response.status().is_client_error();
            let reply: Value = response.json().await?;
            (refused, reply["error"].clone(), reply)
        };
        ensure!(
            refused && error == expected,
            "{case} over {transport}: expected {expected:?}, got {reply}"
        );
        let after = w.get(&format!("address={address}")).await?;
        let rows = saved_rows(w).await?;
        ensure!(
            after["current_revision"] == task["current_revision"]
                && after["process"]["blocked"] == task["process"]["blocked"]
                && rows["position"] == before["position"]
                && rows == before,
            "{case} over {transport} changed saved state: {reply}"
        );
    }
    Ok(())
}

// failed-task-blocks-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn block_and_blocked_task_actions_refuse_exact_cases_over_http_and_mcp() -> Result<()> {
    let w = world().await?;
    // L holds T, M is another member, Z is an absent agent that holds D, and S is the owner.
    let mut tokens = vec![];
    for agent in ["lead", "mate", "zed"] {
        let created = store::create_agent_token(&w.pool, &w.ws, "owner", agent, None).await?;
        tokens.push(created["token"].as_str().unwrap().to_owned());
    }
    let (l, m, z, s) = (&tokens[0], &tokens[1], &tokens[2], &w.owner);
    w.command_as(l, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    for id in ["t", "u", "d"] {
        w.command(json!({"op":"save_object","kind":"task","id":id,"revision":0,"parent":"ac:key-rights","data":{"title":"Complete the defined work","owner":"owner","contract":"Implement the key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Create work"})).await?;
    }
    w.command_as(l, json!({"op":"claim_task","id":"task:t","revision":1}))
        .await?;
    w.command_as(
        l,
        json!({"op":"release_task","id":"task:t","revision":2,"reason":"Return the work"}),
    )
    .await?;
    w.command_as(l, json!({"op":"claim_task","id":"task:t","revision":3}))
        .await?;
    w.command_as(z, json!({"op":"claim_task","id":"task:d","revision":1}))
        .await?;
    let t = w.get("address=task:t").await?;
    ensure!(
        t["current_revision"] == 4 && t["process"]["assignee"] == "lead",
        "{t}"
    );
    let block = |id: &str, revision: u64, reason: &str, evidence: &str| json!({"op":"block_task","id":id,"revision":revision,"reason":reason,"evidence":evidence});
    let (reason, evidence) = ("Verification failed", "test x failed on abc123");

    refuse_task_action(
        &w,
        "M blocks T",
        m,
        block("task:t", 4, reason, evidence),
        "Task holder required",
    )
    .await?;
    refuse_task_action(
        &w,
        "L blocks T at revision 3",
        l,
        block("task:t", 3, reason, evidence),
        "stale Task revision; read the Task again",
    )
    .await?;
    refuse_task_action(
        &w,
        "L blocks T with a blank reason",
        l,
        block("task:t", 4, " ", evidence),
        "text must not be empty",
    )
    .await?;
    refuse_task_action(
        &w,
        "L blocks T with blank evidence",
        l,
        block("task:t", 4, reason, " "),
        "text must not be empty",
    )
    .await?;
    refuse_task_action(
        &w,
        "L blocks U",
        l,
        block("task:u", 1, reason, evidence),
        "Task is not claimed unfinished work",
    )
    .await?;

    w.command_as(l, block("task:t", 4, reason, evidence))
        .await?;
    let t = w.get("address=task:t").await?;
    ensure!(
        t["current_revision"] == 5
            && t["process"]["blocked"]["actor"] == "lead"
            && t["process"]["assignee"].is_null(),
        "{t}"
    );
    refuse_task_action(
        &w,
        "L claims blocked T",
        l,
        json!({"op":"claim_task","id":"task:t","revision":5}),
        "Task is blocked for planning",
    )
    .await?;
    refuse_task_action(&w, "L completes blocked T", l, json!({"op":"complete_task","id":"task:t","revision":5,"evidence":"Implemented and checked"}), "Task is not claimed unfinished work").await?;
    refuse_task_action(
        &w,
        "S releases blocked T",
        s,
        json!({"op":"release_task","id":"task:t","revision":5,"reason":"Return the work"}),
        "Task is blocked for planning",
    )
    .await?;

    w.command_as(
        s,
        block("task:d", 2, "Agent Z is absent", "No heartbeat from zed"),
    )
    .await?;
    let d = w.get("address=task:d").await?;
    ensure!(
        d["process"]["blocked"]["actor"] == "owner" && d["process"]["assignee"].is_null(),
        "{d}"
    );
    store::check_journal(&w.pool).await?;
    Ok(())
}

/// One command over REST or MCP joppa_command: the reply, or the refusal text.
async fn send_over(
    w: &World,
    mcp: bool,
    token: &str,
    command: &Value,
) -> Result<Result<Value, String>> {
    if !mcp {
        let (ok, reply) = send_as(w, token, command).await?;
        return Ok(if ok {
            Ok(reply)
        } else {
            Err(reply["error"].as_str().unwrap_or_default().to_owned())
        });
    }
    let result = mcp_call(
        w,
        token,
        "joppa_command",
        json!({"workspace":w.ws,"request_id":uuid::Uuid::new_v4().to_string(),"command":command}),
    )
    .await?;
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    Ok(if result["isError"] == true {
        Err(text.to_owned())
    } else {
        Ok(serde_json::from_str(text)?)
    })
}

/// The refusal text matches, and the saved rows with the journal stay the same.
async fn refused_without_change(
    w: &World,
    mcp: bool,
    token: &str,
    command: Value,
    error: &str,
) -> Result<()> {
    let before = saved_rows(w).await?;
    let reply = send_over(w, mcp, token, &command).await?;
    ensure!(
        reply.as_ref().err().map(String::as_str) == Some(error),
        "mcp={mcp} {command}: {reply:?}"
    );
    ensure!(
        saved_rows(w).await? == before,
        "mcp={mcp} {command}: a refusal changed state"
    );
    Ok(())
}

/// Every stored revision of one object, in revision order.
async fn object_history(w: &World, address: &str) -> Result<Value> {
    let conn = w.pool.get().await?;
    let rows: String = conn
        .query_one(
            "SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY r.revision),'[]')::text FROM joppa.object_revisions r WHERE workspace=$1 AND address=$2",
            &[&w.ws, &address],
        )
        .await?
        .get(0);
    Ok(serde_json::from_str(&rows)?)
}

/// The newest journal event: its actor and command.
async fn last_event(w: &World) -> Result<(String, Value)> {
    let conn = w.pool.get().await?;
    let row = conn
        .query_one(
            "SELECT actor, command::text FROM joppa.events WHERE workspace=$1 ORDER BY position DESC LIMIT 1",
            &[&w.ws],
        )
        .await?;
    Ok((row.get(0), serde_json::from_str(row.get(1))?))
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_releases_an_absent_workers_task_claim() -> Result<()> {
    // owner-releases-claim-ac1
    for mcp in [false, true] {
        let w = world().await?;
        let p = store::create_agent_token(&w.pool, &w.ws, "owner", "P", None).await?;
        let p = p["token"].as_str().unwrap();
        w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
        // Bot authors T. The human owner alex is neither the holder nor the workspace owner.
        let save = |revision: u32| json!({"op":"save_object","kind":"task","id":"t","revision":revision,"parent":"ac:key-rights","data":{"title":format!("Implement the specified work, draft {revision}"),"owner":"alex","contract":"Implement the criterion and record the result","purpose":"implementation","projects":[]},"reason":"Draft the work"});
        for revision in 0..7 {
            w.command_as(&w.bot, save(revision)).await?;
        }
        w.command_as(p, json!({"op":"claim_task","id":"task:t","revision":7}))
            .await?;
        let held = w.get("address=task:t").await?;
        ensure!(
            held["current_revision"] == 8
                && held["process"]["status"] == "in_progress"
                && held["process"]["assignee"] == "P"
                && held["document"]["data"]["owner"] == "alex",
            "{held}"
        );
        let release = |revision: u32, reason: &str| json!({"op":"release_task","id":"task:t","revision":revision,"reason":reason});

        refused_without_change(
            &w,
            mcp,
            &w.bot,
            release(8, "Worker unavailable"),
            "workspace owner required",
        )
        .await?;
        let after = w.get("address=task:t").await?;
        ensure!(
            after["current_revision"] == 8 && after["process"]["assignee"] == "P",
            "{after}"
        );
        refused_without_change(&w, mcp, &w.owner, release(8, " "), "text must not be empty")
            .await?;
        refused_without_change(
            &w,
            mcp,
            &w.owner,
            release(7, "Worker unavailable"),
            "stale Task revision; read the Task again",
        )
        .await?;

        let reply = send_over(&w, mcp, &w.owner, &release(8, "Worker unavailable")).await?;
        ensure!(reply.is_ok(), "mcp={mcp}: {reply:?}");
        let released = w.get("address=task:t").await?;
        ensure!(
            released["current_revision"] == 9
                && released["process"]["status"] == "open"
                && released["process"]["assignee"].is_null()
                && released["process"]["done"].is_null()
                && released["process"]["retired"].is_null()
                && released["document"]["actor"] == "owner"
                && released["document"]["reason"] == "Worker unavailable"
                && released["document"]["data"] == held["document"]["data"],
            "{released}"
        );
        let (actor, command) = last_event(&w).await?;
        ensure!(
            actor == "owner"
                && command["op"] == "release_task"
                && command["reason"] == "Worker unavailable",
            "{actor}: {command}"
        );
        let available = mcp_value(
            &w,
            "joppa_tasks",
            json!({"workspace":w.ws,"view":"available"}),
        )
        .await?;
        ensure!(
            available["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["address"] == "task:t" && row["claimable"] == true),
            "{available}"
        );
        refused_without_change(
            &w,
            mcp,
            &w.owner,
            release(9, "Worker unavailable"),
            "Task is not claimed unfinished work",
        )
        .await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_owner_releases_an_absent_planners_claim() -> Result<()> {
    // owner-releases-claim-ac2
    for mcp in [false, true] {
        let w = world().await?;
        let p = store::create_agent_token(&w.pool, &w.ws, "owner", "P", None).await?;
        let p = p["token"].as_str().unwrap();
        let worker = store::create_agent_token(&w.pool, &w.ws, "owner", "W", None).await?;
        let worker = worker["token"].as_str().unwrap();
        w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
        let t0 = ready_task(&w, "t0", "key-rights").await?;
        w.command_as(worker, json!({"op":"claim_task","id":t0,"revision":1}))
            .await?;
        let revision = async |address: &str| -> Result<Value> {
            Ok(w.get(&format!("address={address}")).await?["current_revision"].clone())
        };
        w.command_as(p, json!({"op":"claim_planning","ac":"ac:key-rights","revision":revision("ac:key-rights").await?})).await?;
        for id in ["t1", "t2"] {
            w.command_as(p, ready_task_command(id, "key-rights"))
                .await?;
        }
        w.command_as(p, json!({"op":"save_object","kind":"task","id":"t0","revision":revision(&t0).await?,"parent":"ac:key-rights","data":{"title":"Implement the planned work","owner":"owner","contract":"Implement the criterion and record the result","purpose":"implementation","projects":[]},"reason":"Update the older work"})).await?;
        let snapshot = w.get("address=task:t0").await?;
        let history = object_history(&w, "task:t0").await?;
        ensure!(
            snapshot["document"]["actor"] == "P"
                && snapshot["process"]["assignee"] == "W"
                && history.as_array().unwrap().len() as u64
                    == snapshot["current_revision"].as_u64().unwrap(),
            "{snapshot} / {history}"
        );
        let release =
            |reason: &str| json!({"op":"release_planning","ac":"ac:key-rights","reason":reason});

        refused_without_change(
            &w,
            mcp,
            &w.bot,
            release("Planner unavailable"),
            "workspace owner required",
        )
        .await?;
        let claim = w.get("address=ac:key-rights").await?;
        ensure!(
            claim["process"]["planning_claim"]["actor"] == "P",
            "{claim}"
        );
        refused_without_change(&w, mcp, &w.owner, release(" "), "text must not be empty").await?;

        let reply = send_over(&w, mcp, &w.owner, &release("Planner unavailable")).await?;
        ensure!(reply.is_ok(), "mcp={mcp}: {reply:?}");
        let ac = w.get("address=ac:key-rights").await?;
        ensure!(ac["process"]["planning_claim"].is_null(), "{ac}");
        for id in ["task:t1", "task:t2"] {
            let retired = w.get(&format!("address={id}")).await?;
            ensure!(
                retired["process"]["status"] == "retired"
                    && retired["process"]["retired"] == "Planner unavailable"
                    && retired["document"]["present"] == true,
                "{retired}"
            );
        }
        let t0_after = w.get("address=task:t0").await?;
        ensure!(
            t0_after["process"]["status"] == "in_progress"
                && t0_after["process"]["assignee"] == "W"
                && t0_after["document"] == snapshot["document"]
                && t0_after["current_revision"] == snapshot["current_revision"]
                && object_history(&w, "task:t0").await? == history,
            "{snapshot} / {t0_after}"
        );
        let (actor, command) = last_event(&w).await?;
        ensure!(
            actor == "owner"
                && command["op"] == "release_planning"
                && command["reason"] == "Planner unavailable",
            "{actor}: {command}"
        );
        refused_without_change(
            &w,
            mcp,
            &w.owner,
            release("Planner unavailable"),
            "criterion has no planning claim",
        )
        .await?;
    }
    Ok(())
}

// tree-schema-revision-ac1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_owner_schema_save_over_http_adds_one_revision_and_keeps_the_old_one() -> Result<()> {
    async fn tree(w: &World, query: &str) -> Result<Value> {
        let reply = w
            .http
            .get(format!("{}/api/workspaces/{}/tree{query}", w.root, w.ws))
            .bearer_auth(&w.owner)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status == 200, "GET /tree{query}: {status}: {body}");
        Ok(serde_json::from_str(&body)?)
    }
    let w = world().await?;
    let before = tree(&w, "").await?;
    let n = before["current_revision"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("current_revision needs a number: {before}"))?;
    let old_types = before["schema"]["types"].clone();
    ensure!(
        old_types.is_object(),
        "schema.types needs an object: {before}"
    );
    ensure!(
        old_types.get("risk").is_none(),
        "risk already exists: {before}"
    );
    let mut types = old_types.clone();
    types["risk"] = json!({"title":"Risk","role":"document","root":false,"parents":["capability"]});
    let reply = w
        .http
        .post(format!("{}/api/workspaces/{}/commands", w.root, w.ws))
        .bearer_auth(&w.owner)
        .json(&json!({"request_id": uuid::Uuid::new_v4().to_string(), "command": {"op":"set_tree_schema","revision":n,"types":types,"reason":"Add risk"}}))
        .send()
        .await?;
    let status = reply.status();
    let body = reply.text().await?;
    ensure!(status == 200, "set_tree_schema: {status}: {body}");
    let current = tree(&w, "").await?;
    let old = tree(&w, &format!("?revision={n}")).await?;
    println!("current: {current}\nrevision {n}: {old}");
    ensure!(
        current["current_revision"] == n + 1,
        "current_revision is not {}: {current}\nrevision {n}: {old}",
        n + 1
    );
    ensure!(
        current["schema"]["types"]["risk"]["title"] == "Risk",
        "the new type is missing: {current}\nrevision {n}: {old}"
    );
    ensure!(
        old["schema"]["revision"] == n,
        "schema.revision is not {n}: {current}\nrevision {n}: {old}"
    );
    ensure!(
        old["schema"]["types"] == old_types,
        "revision {n} lost its old types: {current}\nrevision {n}: {old}"
    );
    Ok(())
}

// tree-schema-revision-ac2
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_owner_schema_save_on_a_stale_revision_over_http_is_refused_and_changes_nothing()
-> Result<()> {
    async fn tree(w: &World) -> Result<Value> {
        let reply = w
            .http
            .get(format!("{}/api/workspaces/{}/tree", w.root, w.ws))
            .bearer_auth(&w.owner)
            .send()
            .await?;
        let status = reply.status();
        let body = reply.text().await?;
        ensure!(status == 200, "GET /tree: {status}: {body}");
        Ok(serde_json::from_str(&body)?)
    }
    let w = world().await?;
    let before = tree(&w).await?;
    let n = before["current_revision"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("current_revision needs a number: {before}"))?;
    let mut risk = before["schema"]["types"].clone();
    ensure!(risk.get("risk").is_none(), "risk already exists: {before}");
    let mut job = risk.clone();
    risk["risk"] = json!({"title":"Risk","role":"document","root":false,"parents":["capability"]});
    job["job"] = json!({"title":"Jobs","role":"task","root":true,"parents":["*"]});
    w.command(json!({"op":"set_tree_schema","revision":n,"types":risk,"reason":"Add risk"}))
        .await?;
    let control = tree(&w).await?;
    ensure!(
        control["current_revision"] == n + 1,
        "current_revision is not {}: {control}",
        n + 1
    );
    let body = w
        .refused(json!({"op":"set_tree_schema","revision":n,"types":job,"reason":"Add jobs"}))
        .await?;
    let reply: Value = serde_json::from_str(&body)?;
    ensure!(
        reply["error"] == "stale schema revision; read the tree schema again",
        "{body}"
    );
    let after = tree(&w).await?;
    ensure!(
        after["current_revision"] == n + 1,
        "current_revision is not {}: {after}",
        n + 1
    );
    ensure!(
        after == control,
        "a stale save changed the tree:\n{after}\ncontrol:\n{control}"
    );
    Ok(())
}
