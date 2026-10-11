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
    /// A signed-in account that is not a member of the workspace.
    eve: String,
    pool: deadpool_postgres::Pool,
    http: reqwest::Client,
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
    for (i, op) in [
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
    .enumerate()
    {
        let op: Operation = serde_json::from_value(op)?;
        store::execute(&pool, &ws, "owner", &format!("setup-{i}"), &op).await?;
    }
    let dir = std::env::temp_dir().join(format!("joppa-spec-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let owner = uuid::Uuid::new_v4().simple().to_string();
    let alex = uuid::Uuid::new_v4().simple().to_string();
    let maria = uuid::Uuid::new_v4().simple().to_string();
    let bot = uuid::Uuid::new_v4().simple().to_string();
    let eve = uuid::Uuid::new_v4().simple().to_string();
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({ owner.clone(): "owner", alex.clone(): "alex", maria.clone(): "maria", bot.clone(): "bot", eve.clone(): "eve" }).to_string(),
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
        let reply = self
            .http
            .get(format!(
                "{}/api/workspaces/{}/objects?{query}",
                self.root, self.ws
            ))
            .bearer_auth(&self.owner)
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
async fn a_previous_assignee_cannot_complete_returned_work() -> Result<()> {
    // old-agent-locked-out-ac1
    let w = world().await?;
    let first = store::create_agent_token(&w.pool, &w.ws, "owner", "bot").await?;
    let second = store::create_agent_token(&w.pool, &w.ws, "owner", "bot2").await?;
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
    w.command(json!({"op":"save_object","kind":"task","id":"owner-completion","revision":0,"parent":"ac:key-rights","data":{"title":"Record the owner completion","owner":"owner","contract":"Implement the same key rights contract","purpose":"implementation","projects":[]},"reason":"Check the existing owner authority"})).await?;
    w.command_as(
        bot2,
        json!({"op":"claim_task","id":"task:owner-completion","revision":1}),
    )
    .await?;
    w.command(json!({"op":"complete_task","id":"task:owner-completion","revision":2,"evidence":"The owner records the completed work"})).await?;
    let owned = w.get("address=task:owner-completion").await?;
    ensure!(
        owned["document"]["actor"] == "owner" && owned["process"]["done"].is_string(),
        "{owned}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_agent_cannot_set_task_verification_by_hand() -> Result<()> {
    // done-is-not-green-ac2
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot").await?;
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
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot").await?;
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
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot").await?;
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
async fn returned_work_keeps_notes_and_partial_results_for_the_next_agent() -> Result<()> {
    // returned-work-kept-ac1
    let w = world().await?;
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot2").await?;
    let bot2 = token["token"].as_str().unwrap();
    w.command_as(&w.bot, json!({"op":"assess","req":"key","revision":1,"ac":"key-rights","assessment":{"ready":true,"reason":"The contract is clear","recipient":null,"decision":null}})).await?;
    w.command(json!({"op":"save_object","kind":"task","id":"handover","revision":0,"parent":"ac:key-rights","data":{"title":"Implement the key contract","owner":"owner","contract":"Implement the exact key rights and record the checks","purpose":"implementation","projects":[]},"reason":"Create work for the handover"})).await?;
    w.command_as(
        &w.bot,
        json!({"op":"claim_task","id":"task:handover","revision":1}),
    )
    .await?;
    for (id, text) in [
        ("first", "The first boundary is clear"),
        ("second", "The second boundary still needs work"),
    ] {
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
    let before = w.get("address=task:handover").await?;
    w.command_as(&w.bot, json!({"op":"release_task","id":"task:handover","revision":before["current_revision"],"reason":"Return the remaining boundary to the queue"})).await?;
    let available = w
        .http
        .post(format!("{}/mcp", w.root))
        .bearer_auth(bot2)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_tasks","arguments":{"workspace":w.ws,"view":"available"}}}))
        .send()
        .await?;
    ensure!(
        available.status().is_success(),
        "{}: {}",
        available.status(),
        available.text().await?
    );
    let reply: Value = available.json().await?;
    ensure!(reply["result"]["isError"] != true, "{reply}");
    let available: Value = serde_json::from_str(
        reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("null"),
    )?;
    ensure!(
        available["tasks"]
            .as_array()
            .is_some_and(|tasks| tasks.iter().any(|task| task["address"] == "task:handover")),
        "{available}"
    );
    let returned = w.get("address=task:handover").await?;
    w.command_as(
        bot2,
        json!({"op":"claim_task","id":"task:handover","revision":returned["current_revision"]}),
    )
    .await?;

    let reply = w.http.post(format!("{}/mcp", w.root)).bearer_auth(bot2).json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":w.ws,"address":"task:handover"}}})).send().await?;
    ensure!(reply.status().is_success(), "{}", reply.text().await?);
    let reply: Value = reply.json().await?;
    ensure!(reply["result"]["isError"] != true, "{reply}");
    let task: Value = serde_json::from_str(
        reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("null"),
    )?;
    ensure!(task["process"]["assignee"] == "bot2", "{task}");
    ensure!(task["process"]["status"] == "in_progress", "{task}");
    ensure!(
        task["annotations"] == before["annotations"],
        "the handover changed the original notes: {task}"
    );
    let notes = task["annotations"]["comments"].as_object().unwrap();
    ensure!(
        notes.len() == 2 && notes.values().all(|note| note["author"] == "bot"),
        "{notes:?}"
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
    w.note("input", 0, "Input", "Context", json!([])).await?;
    for address in [
        "domain:security",
        "capability:restrict",
        "requirement:key",
        "ac:key-rights",
        "note:input",
    ] {
        let before = chrono::Utc::now();
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
        ensure!(
            created >= before && created <= chrono::Utc::now(),
            "{comment}"
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
    w.note("a", 0, "A", "first text", json!([])).await?;
    w.note("a", 1, "A", "second text", json!([])).await?;
    w.note("a", 2, "A", "third text", json!([])).await?;
    let old = w.get("address=note:a&revision=1").await?;
    ensure!(
        old["document"]["revision"] == 1 && old["document"]["data"]["body"] == "first text",
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
    let list = w.get("limit=50&parent=domain:security").await?;
    let addresses: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["address"].as_str())
        .collect();
    ensure!(
        addresses.contains(&"capability:restrict") && !addresses.contains(&"capability:empty"),
        "{addresses:?}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn an_archived_object_still_reads_by_address() -> Result<()> {
    // removal-is-archiving-ac4
    let w = world().await?;
    let cap = w.get("address=capability:empty").await?;
    w.command(json!({"op":"archive_object","target":"capability:empty","revision":cap["document"]["revision"],"reason":"Cut"})).await?;
    let read = w.get("address=capability:empty").await?;
    ensure!(read["document"]["present"] == false, "{read}");
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
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn rename_and_move_keep_the_address_and_the_item_id() -> Result<()> {
    // id-survives-move-ac1
    let w = world().await?;
    let before = w.get("address=requirement:key").await?;
    let mut data = before["document"]["data"].clone();
    data["title"] = json!("Key rights, renamed");
    w.command(json!({"op":"save_object","kind":"requirement","id":"key","revision":before["document"]["revision"],"parent":"capability:empty","data":data,"links":[],"reason":"Rename and move"})).await?;
    let after = w.get("address=requirement:key").await?;
    ensure!(
        after["address"] == before["address"]
            && after["item_id"] == before["item_id"]
            && after["document"]["parent"] == "capability:empty"
            && after["document"]["data"]["title"] == "Key rights, renamed",
        "{before} -> {after}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_blocking_link_pins_the_target_revision_and_asks_for_review() -> Result<()> {
    // blocking-pins-revision-ac1
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
            && source["review_required"] == true,
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

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn search_omits_archived_objects() -> Result<()> {
    // search-respects-access-ac1
    let w = world().await?;
    w.note("old", 0, "Legacy flow", "legacy", json!([])).await?;
    let read = w.get("address=note:old").await?;
    w.command(json!({"op":"archive_object","target":"note:old","revision":read["document"]["revision"],"reason":"Cut"})).await?;
    let found = w.search("legacy").await?;
    ensure!(!found.contains(&"note:old".into()), "{found:?}");
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
    let found = a.search("passkey").await?;
    ensure!(found.is_empty(), "{found:?}");
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
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot").await?;
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
    let history = w.get("address=note:original&revision=1").await?;
    ensure!(
        history["document"] == original["document"],
        "The source rule changed a historical revision: {history}"
    );
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot").await?;
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
                    status == 409 && denied["error"] == "a signal needs a source",
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
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_inbox_lists_undecided_inputs_before_decided_inputs() -> Result<()> {
    // undecided-first-ac1
    let w = world().await?;
    let grant = store::create_agent_token(&w.pool, &w.ws, "owner", "bot").await?;
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

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_signal_appears_in_the_inbox_list() -> Result<()> {
    // signal-in-one-inbox-ac1
    let w = world().await?;
    w.note("first", 0, "First", "Refunds are slow", json!([]))
        .await?;
    let inbox = w.get("kind=note&parent=*&limit=100").await?;
    let items = inbox["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        items.len() == 1 && items[0]["address"] == "note:first",
        "{inbox}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_note_under_any_object_also_appears_in_the_inbox() -> Result<()> {
    // signal-in-one-inbox-ac2
    let w = world().await?;
    w.command(json!({"op":"save_object","kind":"note","id":"under","revision":0,"parent":"requirement:key","data":{"title":"Under","owner":"owner","body":"About keys"},"links":[],"reason":"Save"}))
        .await?;
    let inbox = w.get("kind=note&parent=*&limit=100").await?;
    let items = inbox["items"].as_array().cloned().unwrap_or_default();
    ensure!(
        items
            .iter()
            .any(|i| i["address"] == "note:under" && i["parent"] == "requirement:key"),
        "{inbox}"
    );
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
async fn a_signal_keeps_its_channel_author_and_time() -> Result<()> {
    // signal-records-source-ac1
    let w = world().await?;
    let source = json!({"channel_type":"chat","channel_name":"support","author":"maria","at":"2026-10-09T10:00:00Z"});
    w.command(json!({"op":"save_object","kind":"note","id":"chat","revision":0,"parent":null,"data":{"title":"Refunds are slow","owner":"owner","body":"My refund takes a week","source":source},"links":[],"reason":"Intake"}))
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
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot").await?;
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
    let token = store::create_agent_token(&w.pool, &w.ws, "owner", "bot").await?;
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
        w.command_as(&w.alex, json!({"op":"save_object","kind":object["kind"],"id":object["id"],"revision":1,"parent":object["document"]["parent"],"data":object["document"]["data"],"reason":"Create a later document revision"})).await?;
        let historical = w.get(&format!("address={address}&revision=1")).await?;
        ensure!(
            historical["historical"] == true && historical.get("files").is_none(),
            "A saved document exposes the current files: {historical}"
        );
    }
    Ok(())
}
