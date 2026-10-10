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
    let eve = uuid::Uuid::new_v4().simple().to_string();
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        json!({ owner.clone(): "owner", eve.clone(): "eve" }).to_string(),
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
    let by_id = w.search(&item_id).await?;
    ensure!(
        by_id.first().map(String::as_str) == Some("note:second"),
        "{item_id}: {by_id:?}"
    );
    let by_address = w.search("note:second").await?;
    ensure!(
        by_address.first().map(String::as_str) == Some("note:second"),
        "{by_address:?}"
    );
    Ok(())
}
