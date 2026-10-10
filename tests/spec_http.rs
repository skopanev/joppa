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
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, json!({ owner.clone(): "owner" }).to_string())?;
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
        let reply = self
            .http
            .post(format!("{}/api/workspaces/{}/commands", self.root, self.ws))
            .bearer_auth(&self.owner)
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
