//! A node can stand for another workspace, and an object can link to an object of another
//! workspace, against a real server. Workspace P is the product, workspace C a component.
//! Owner and agent bot belong to both. Member alex belongs only to P.
use anyhow::{Context, Result, ensure};
use joppa::store;
use serde_json::{Value, json};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const TEST: &str = "tests/api.rs > api";

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
    root: String,
    p: String,
    c: String,
    owner: String,
    bot: String,
    alex: String,
    pool: deadpool_postgres::Pool,
    http: reqwest::Client,
}

const TYPES: &str = r#"{
    "component":{"title":"Components","role":"group","status":"children","root":true,"parents":[]},
    "requirement":{"title":"Requirements","role":"requirement","status":"children","root":true,"parents":[]},
    "ac":{"title":"Criteria","role":"ac","status":"tests","parents":["requirement"]},
    "task":{"title":"Tasks","role":"task","status":"none","parents":["ac"]}
}"#;

async fn workspace(pool: &deadpool_postgres::Pool, prefix: &str, alex: bool) -> Result<String> {
    let ws = format!("{prefix}-{}", uuid::Uuid::new_v4().simple());
    let write = async |actor: &str, command: Value| {
        let id = uuid::Uuid::new_v4().to_string();
        store::execute_remote_input(pool, &ws, actor, &id, &command).await
    };
    let op: joppa::commands::Command =
        serde_json::from_value(json!({"op":"workspace","title":prefix}))?;
    store::execute(pool, &ws, "owner", "setup", &op).await?;
    if alex {
        write("owner", json!({"op":"member","id":"alex","role":"human"})).await?;
    }
    store::enroll_service(pool, &ws, "owner", "bot", "agent").await?;
    let tree = store::tree_schema(pool, &ws, "owner", None).await?;
    write("owner", json!({"op":"set_tree_schema","revision":tree["schema"]["revision"],"reason":"Configure","types":serde_json::from_str::<Value>(TYPES)?})).await?;
    Ok(ws)
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
    let p = workspace(&pool, "product", true).await?;
    let c = workspace(&pool, "component", false).await?;
    let dir = std::env::temp_dir().join(format!("joppa-nw-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir)?;
    let token = || uuid::Uuid::new_v4().simple().to_string();
    let (owner, bot, alex) = (token(), token(), token());
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
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("JOPPA_DATABASE_URL", &url)
            .env("JOPPA_TOKENS_FILE", &tokens)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .env("DEEPINFRA_API_KEY", "test-only-key")
            .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    for _ in 0..100 {
        if reqwest::get(format!("{root}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            let w = World {
                _server: server,
                _dir: dir,
                root,
                p,
                c,
                owner,
                bot,
                alex,
                pool,
                http: reqwest::Client::new(),
            };
            w.fixture().await?;
            return Ok(w);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}

impl World {
    /// One MCP tool call. A refusal gives Err with the exact error text.
    async fn mcp(&self, token: &str, tool: &str, args: Value) -> Result<Result<Value, String>> {
        let reply: Value = self
            .http
            .post(format!("{}/mcp", self.root))
            .bearer_auth(token)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}}))
            .send()
            .await?
            .json()
            .await?;
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .with_context(|| format!("{reply}"))?;
        Ok(if reply["result"]["isError"] == true {
            Err(text.to_owned())
        } else {
            Ok(serde_json::from_str(text)?)
        })
    }
    async fn command(
        &self,
        token: &str,
        ws: &str,
        command: Value,
    ) -> Result<Result<Value, String>> {
        let id = uuid::Uuid::new_v4().to_string();
        self.mcp(
            token,
            "joppa_command",
            json!({"workspace":ws,"request_id":id,"command":command}),
        )
        .await
    }
    async fn ok(&self, token: &str, ws: &str, command: Value) -> Result<Value> {
        self.command(token, ws, command.clone())
            .await?
            .map_err(|e| anyhow::anyhow!("{command}: {e}"))
    }
    async fn refused(&self, token: &str, ws: &str, command: Value, error: &str) -> Result<()> {
        match self.command(token, ws, command.clone()).await? {
            Err(e) if e.contains(error) => Ok(()),
            other => anyhow::bail!("{command}: expected {error:?}, got {other:?}"),
        }
    }
    async fn object(&self, token: &str, ws: &str, address: &str) -> Result<Value> {
        self.mcp(
            token,
            "joppa_objects",
            json!({"workspace":ws,"address":address}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{address}: {e}"))
    }
    async fn record_in(&self, ws: &str, commit: &str, status: &str) -> Result<()> {
        self.mcp(
            &self.bot,
            "joppa_record_results",
            json!({"workspace":ws,"commit":commit,"results":[{"test":TEST,"status":status}]}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("record: {e}"))?;
        Ok(())
    }
    async fn record(&self, commit: &str, status: &str) -> Result<()> {
        self.mcp(
            &self.bot,
            "joppa_record_results",
            json!({"workspace":self.c,"commit":commit,"results":[{"test":TEST,"status":status}]}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("record: {e}"))?;
        Ok(())
    }
    fn save(kind: &str, id: &str, revision: u64, parent: Option<&str>, data: Value) -> Value {
        json!({"op":"save_object","kind":kind,"id":id,"revision":revision,"parent":parent,"data":data,"reason":"Fixture"})
    }
    /// P: requirement R with criterion A, ready for planning. C: requirement API with
    /// criterion X that lists TEST.
    async fn fixture(&self) -> Result<()> {
        let (owner, p, c) = (&self.owner, &self.p, &self.c);
        self.ok(
            owner,
            p,
            Self::save(
                "requirement",
                "r",
                0,
                None,
                json!({"title":"R","owner":"owner","body":"The product rule"}),
            ),
        )
        .await?;
        self.ok(
            owner,
            p,
            Self::save(
                "ac",
                "a",
                0,
                Some("requirement:r"),
                json!({"title":"A","owner":"owner"}),
            ),
        )
        .await?;
        let a = self.object(owner, p, "ac:a").await?;
        self.ok(owner, p, json!({"op":"assess","req":a["process"]["req"],"revision":a["process"]["revision"],"ac":a["process"]["criterion"],
            "assessment":{"ready":true,"reason":"The scope of A is known"}})).await?;
        self.ok(
            owner,
            c,
            Self::save(
                "requirement",
                "api",
                0,
                None,
                json!({"title":"API","owner":"owner","body":"The API contract"}),
            ),
        )
        .await?;
        self.ok(
            owner,
            c,
            Self::save(
                "ac",
                "x",
                0,
                Some("requirement:api"),
                json!({"title":"X","owner":"owner"}),
            ),
        )
        .await?;
        self.ok(
            owner,
            c,
            json!({"op":"set_ac_tests","target":"ac:x","tests":[TEST],"reason":"Bind"}),
        )
        .await?;
        Ok(())
    }
    fn link(&self, revision: Option<u64>, blocking: bool) -> Value {
        let mut link = json!({"workspace":self.c,"target":"requirement:api","relation":if blocking {"depends_on"} else {"related"},"blocking":blocking});
        if let Some(r) = revision {
            link["revision"] = json!(r);
        }
        link
    }
    /// The current revision of an object.
    async fn revision(&self, ws: &str, address: &str) -> Result<u64> {
        Ok(
            self.object(&self.owner, ws, address).await?["current_revision"]
                .as_u64()
                .unwrap_or_default(),
        )
    }
    /// Replace the remote links of one object, with the links revision that it reads first.
    async fn set_links(
        &self,
        token: &str,
        ws: &str,
        target: &str,
        links: Value,
    ) -> Result<Result<Value, String>> {
        let current = self.object(&self.owner, ws, target).await?["remote_links_revision"].clone();
        self.command(token, ws, json!({"op":"set_remote_links","target":target,"revision":current,"links":links,"reason":"Link to another workspace"})).await
    }
    /// A depends on the current revision of API with a blocking link, and A is ready again.
    async fn block_a_on_api(&self) -> Result<()> {
        let pin = self.revision(&self.c, "requirement:api").await?;
        self.set_links(
            &self.bot,
            &self.p,
            "ac:a",
            json!([self.link(Some(pin), true)]),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        let a = self.object(&self.owner, &self.p, "ac:a").await?;
        self.ok(&self.owner, &self.p, json!({"op":"assess","req":a["process"]["req"],"revision":a["process"]["revision"],"ac":a["process"]["criterion"],
            "assessment":{"ready":true,"reason":"The scope of A is known"}})).await?;
        Ok(())
    }
    fn waiting(&self) -> String {
        format!("waits for @{}/requirement:api, which is not green", self.c)
    }
    async fn claim(&self) -> Result<Result<Value, String>> {
        let a = self.object(&self.bot, &self.p, "ac:a").await?;
        self.command(
            &self.bot,
            &self.p,
            json!({"op":"claim_planning","ac":"ac:a","revision":a["current_revision"]}),
        )
        .await
    }
}

// node-workspace-ac1
#[tokio::test]
async fn a_node_shows_the_child_workspace_status_with_the_access_of_the_reader() -> Result<()> {
    let w = world().await?;
    let node = |ws: &str| json!({"title":"Backend","owner":"owner","workspace":ws});
    w.ok(
        &w.owner,
        &w.p,
        World::save("component", "backend", 0, None, node(&w.c)),
    )
    .await?;
    let read = w.object(&w.owner, &w.p, "component:backend").await?;
    ensure!(
        read["child_workspace"]
            == json!({"workspace":w.c,"access":true,"status":"grey","criteria":{"green":0,"red":0,"total":1}}),
        "{read}"
    );
    w.record("1111111111111111111111111111111111111111", "pass")
        .await?;
    let read = w.object(&w.owner, &w.p, "component:backend").await?;
    ensure!(read["child_workspace"]["status"] == "green", "{read}");
    // A workspace name is stored as the immutable ID, and reads name that ID.
    let alias = format!("renamed-{}", uuid::Uuid::new_v4().simple());
    w.ok(
        &w.owner,
        &w.c,
        json!({"op":"rename_workspace","workspace_id":alias,"title":"Backend","reason":"New name"}),
    )
    .await?;
    w.ok(
        &w.owner,
        &w.p,
        World::save("component", "mobile", 0, None, node(&alias)),
    )
    .await?;
    let mobile = w.object(&w.owner, &w.p, "component:mobile").await?;
    ensure!(
        mobile["document"]["data"]["workspace"] == json!(w.c)
            && mobile["child_workspace"]["workspace"] == json!(w.c),
        "{mobile}"
    );
    ensure!(w.object(&w.owner, &w.c, "requirement:api").await?["workspace_key"] == json!(w.c));
    let tree = w
        .mcp(&w.owner, "joppa_tree", json!({"workspace":alias}))
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    ensure!(
        tree["workspace_key"] == json!(w.c) && tree["workspace"] == json!(alias),
        "{tree}"
    );
    let alex = w.object(&w.alex, &w.p, "component:backend").await?;
    ensure!(
        alex["child_workspace"] == json!({"workspace":w.c,"access":false,"status":"grey"}),
        "a reader without access sees no data: {alex}"
    );
    // The own workspace, an unknown workspace and a workspace without access are refused.
    w.refused(
        &w.owner,
        &w.p,
        World::save("component", "own", 0, None, node(&w.p)),
        "names another workspace",
    )
    .await?;
    w.refused(
        &w.owner,
        &w.p,
        World::save("component", "none", 0, None, node("missing-workspace")),
        "no read access to workspace missing-workspace",
    )
    .await?;
    w.refused(
        &w.alex,
        &w.p,
        World::save(
            "component",
            "alex",
            0,
            None,
            json!({"title":"Backend","owner":"alex","workspace":w.c}),
        ),
        &format!("no read access to workspace {}", w.c),
    )
    .await?;
    Ok(())
}

// node-workspace-ac2
#[tokio::test]
async fn a_remote_link_checks_its_target_and_shows_a_changed_contract() -> Result<()> {
    let w = world().await?;
    let pin = w.revision(&w.c, "requirement:api").await?;
    w.set_links(&w.bot, &w.p, "ac:a", json!([w.link(Some(pin), false)]))
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let read = w.object(&w.bot, &w.p, "ac:a").await?;
    ensure!(
        read["process"]["assessment"]["state"] == "unassessed",
        "a new pin resets the criteria below: {read}"
    );
    ensure!(
        read["remote_links_revision"] == 1 && read["remote_review_required"] == false,
        "{read}"
    );
    let status = &read["remote_links"][0]["status"];
    ensure!(
        *status
            == json!({"access":true,"present":true,"current_revision":pin,"contract_revision":pin,"review_required":false,"satisfied":false}),
        "{read}"
    );
    // C changes its contract: the pinned link now asks for a review.
    w.ok(
        &w.owner,
        &w.c,
        World::save(
            "requirement",
            "api",
            pin,
            None,
            json!({"title":"API","owner":"owner","body":"The API contract, version 2"}),
        ),
    )
    .await?;
    let read = w.object(&w.bot, &w.p, "ac:a").await?;
    ensure!(
        read["remote_links"][0]["status"]["review_required"] == true
            && read["remote_review_required"] == true,
        "{read}"
    );
    let alex = w.object(&w.alex, &w.p, "ac:a").await?;
    ensure!(
        alex["remote_links"][0]["status"]
            == json!({"access":false,"present":null,"current_revision":null,"review_required":null,"satisfied":false}),
        "{alex}"
    );
    // A stale links revision, a future pin, an unknown target, the own workspace and no access.
    let stale =
        json!({"op":"set_remote_links","target":"ac:a","revision":0,"links":[],"reason":"Stale"});
    w.refused(&w.bot, &w.p, stale, "stale remote links revision")
        .await?;
    let mut bad = w.link(Some(pin + 9), false);
    let refuse =
        async |links: Value, error: &str| match w.set_links(&w.bot, &w.p, "ac:a", links).await? {
            Err(e) if e.contains(error) => anyhow::Ok(()),
            other => anyhow::bail!("expected {error:?}, got {other:?}"),
        };
    refuse(json!([bad.clone()]), "unknown linked revision").await?;
    bad["revision"] = json!(1);
    bad["target"] = json!("requirement:none");
    refuse(json!([bad.clone()]), "unknown or archived object").await?;
    bad["target"] = json!("requirement:r");
    bad["workspace"] = json!(w.p);
    refuse(json!([bad]), "names another workspace").await?;
    match w
        .set_links(&w.alex, &w.p, "ac:a", json!([w.link(Some(1), false)]))
        .await?
    {
        Err(e) if e.contains(&format!("no read access to workspace {}", w.c)) => {}
        other => anyhow::bail!("alex: {other:?}"),
    }
    Ok(())
}

// node-workspace-ac2
#[tokio::test]
async fn an_archived_target_is_shown_for_review_and_a_batch_writes_links_atomically() -> Result<()>
{
    let w = world().await?;
    let pin = w.revision(&w.c, "ac:x").await?;
    // One batch saves a note and its remote link; a refused link rolls back the note.
    let batch = |id: &str, target: &str| {
        json!({"workspace":w.p,"request_id":uuid::Uuid::new_v4().to_string(),"operations":[
        {"command":{"op":"save_object","kind":"requirement","id":id,"revision":0,"parent":null,"data":{"title":"Mobile","owner":"owner","body":"Uses the API"},"reason":"Fixture"}},
        {"command":{"op":"set_remote_links","target":format!("requirement:{id}"),"revision":0,"links":[{"workspace":w.c,"target":target,"relation":"related","revision":pin}],"reason":"Mobile uses X"}}]})
    };
    w.mcp(&w.bot, "joppa_batch", batch("mobile", "ac:x"))
        .await?
        .map_err(|e| anyhow::anyhow!("batch: {e}"))?;
    ensure!(
        w.mcp(&w.bot, "joppa_batch", batch("broken", "ac:none"))
            .await?
            .is_err(),
        "a bad link refuses the batch"
    );
    ensure!(
        w.mcp(
            &w.bot,
            "joppa_objects",
            json!({"workspace":w.p,"address":"requirement:broken"})
        )
        .await?
        .is_err(),
        "the refused batch saved nothing"
    );
    w.ok(
        &w.owner,
        &w.c,
        json!({"op":"archive_object","target":"ac:x","revision":pin,"reason":"X is retired"}),
    )
    .await?;
    let read = w.object(&w.bot, &w.p, "requirement:mobile").await?;
    ensure!(
        read["remote_review_required"] == true
            && read["remote_links"][0]["status"]
                == json!({"access":true,"present":false,"current_revision":null,"review_required":true,"satisfied":false}),
        "{read}"
    );
    Ok(())
}

// node-workspace-ac3
#[tokio::test]
async fn work_waits_for_a_blocking_remote_link_until_its_target_is_green() -> Result<()> {
    let w = world().await?;
    w.block_a_on_api().await?;
    match w.claim().await? {
        Err(e) if e.contains(&w.waiting()) => {}
        other => anyhow::bail!("expected the wait, got {other:?}"),
    }
    w.record("2222222222222222222222222222222222222222", "pass")
        .await?;
    w.claim()
        .await?
        .map_err(|e| anyhow::anyhow!("claim after green: {e}"))?;
    Ok(())
}

// node-workspace-ac3
#[tokio::test]
async fn a_green_target_with_a_newer_revision_does_not_free_work_until_a_new_pin() -> Result<()> {
    let w = world().await?;
    w.block_a_on_api().await?;
    w.record("6666666666666666666666666666666666666666", "pass")
        .await?;
    let pin = w.revision(&w.c, "requirement:api").await?;
    w.ok(
        &w.owner,
        &w.c,
        World::save(
            "requirement",
            "api",
            pin,
            None,
            json!({"title":"API","owner":"owner","body":"The API contract, version 2"}),
        ),
    )
    .await?;
    // The text change of API makes X grey until a new record, so record again: X is green.
    w.record("7777777777777777777777777777777777777777", "pass")
        .await?;
    let read = w.object(&w.bot, &w.p, "ac:a").await?;
    ensure!(
        read["remote_links"][0]["status"]["review_required"] == true
            && read["remote_links"][0]["status"]["satisfied"] == false,
        "{read}"
    );
    match w.claim().await? {
        Err(e) if e.contains(&w.waiting()) => {}
        other => anyhow::bail!("a stale pin must not free work: {other:?}"),
    }
    w.block_a_on_api().await?;
    w.claim()
        .await?
        .map_err(|e| anyhow::anyhow!("claim after the new pin: {e}"))?;
    Ok(())
}

// node-workspace-ac3
#[tokio::test]
async fn a_red_dependency_of_a_green_target_keeps_work_waiting() -> Result<()> {
    let w = world().await?;
    // Q is a third workspace. API in C depends on CORE in Q; A in P depends on API.
    let q = workspace(&w.pool, "core", false).await?;
    w.ok(
        &w.owner,
        &q,
        World::save(
            "requirement",
            "core",
            0,
            None,
            json!({"title":"Core","owner":"owner","body":"The core rule"}),
        ),
    )
    .await?;
    w.ok(
        &w.owner,
        &q,
        World::save(
            "ac",
            "y",
            0,
            Some("requirement:core"),
            json!({"title":"Y","owner":"owner"}),
        ),
    )
    .await?;
    w.ok(
        &w.owner,
        &q,
        json!({"op":"set_ac_tests","target":"ac:y","tests":[TEST],"reason":"Bind"}),
    )
    .await?;
    let core = w.revision(&q, "requirement:core").await?;
    let to_core = json!([{"workspace":q,"target":"requirement:core","relation":"depends_on","revision":core,"blocking":true}]);
    w.set_links(&w.owner, &w.c, "requirement:api", to_core)
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    w.block_a_on_api().await?;

    w.record_in(&w.c, "8888888888888888888888888888888888888888", "pass")
        .await?;
    w.record_in(&q, "9999999999999999999999999999999999999999", "fail")
        .await?;
    // X in C is green, but CORE in Q is red, so API is not ready and A waits.
    match w.claim().await? {
        Err(e) if e.contains(&w.waiting()) => {}
        other => anyhow::bail!("a red transitive dependency must hold A: {other:?}"),
    }
    w.record_in(&q, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "pass")
        .await?;
    w.claim()
        .await?
        .map_err(|e| anyhow::anyhow!("claim after CORE is green: {e}"))?;
    Ok(())
}

// node-workspace-ac4
#[tokio::test]
async fn a_blocking_cycle_across_workspaces_is_refused() -> Result<()> {
    let w = world().await?;
    w.block_a_on_api().await?;
    let a = w.revision(&w.p, "ac:a").await?;
    let back = json!([{"workspace":w.p,"target":"ac:a","relation":"depends_on","revision":a,"blocking":true}]);
    match w.set_links(&w.bot, &w.c, "requirement:api", back).await? {
        Err(e) if e.contains("dependency cycle across workspaces") => Ok(()),
        other => anyhow::bail!("expected the cycle refusal, got {other:?}"),
    }
}

// node-workspace-ac5
#[tokio::test]
async fn a_claim_waits_for_a_result_write_in_the_target_workspace() -> Result<()> {
    let w = world().await?;
    w.block_a_on_api().await?;
    w.record("3333333333333333333333333333333333333333", "pass")
        .await?;
    // A transaction holds the lock of C, as a record of results does while it writes.
    let mut conn = w.pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&w.c],
    )
    .await?;
    let claim = tokio::spawn({
        let (root, p, bot) = (w.root.clone(), w.p.clone(), w.bot.clone());
        async move {
            let http = reqwest::Client::new();
            let read: Value = http.post(format!("{root}/mcp")).bearer_auth(&bot)
                .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"joppa_objects","arguments":{"workspace":p,"address":"ac:a"}}}))
                .send().await?.json().await?;
            let a: Value = serde_json::from_str(
                read["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or("null"),
            )?;
            let reply: Value = http.post(format!("{root}/mcp")).bearer_auth(&bot)
                .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"joppa_command","arguments":{"workspace":p,"request_id":uuid::Uuid::new_v4().to_string(),"command":{"op":"claim_planning","ac":"ac:a","revision":a["current_revision"]}}}}))
                .send().await?.json().await?;
            anyhow::Ok(reply)
        }
    });
    tokio::time::sleep(Duration::from_millis(600)).await;
    ensure!(
        !claim.is_finished(),
        "the claim must wait for the lock of C"
    );
    tx.commit().await?;
    let reply = claim.await??;
    ensure!(
        reply["result"]["isError"] != true,
        "the claim finishes after the lock: {reply}"
    );
    Ok(())
}

// node-workspace-ac5
#[tokio::test]
async fn a_parallel_claim_and_failing_record_finish_in_one_consistent_order() -> Result<()> {
    let w = world().await?;
    w.block_a_on_api().await?;
    w.record("4444444444444444444444444444444444444444", "pass")
        .await?;
    let (claim, record) = tokio::join!(
        w.claim(),
        w.record("5555555555555555555555555555555555555555", "fail")
    );
    record?;
    let claim = claim?;
    ensure!(
        claim.is_ok() || claim.as_ref().is_err_and(|e| e.contains(&w.waiting())),
        "the claim either ran first or saw the red target: {claim:?}"
    );
    // After both, the target is red, so a new claim waits.
    let status = w.object(&w.bot, &w.p, "ac:a").await?["remote_links"][0]["status"].clone();
    ensure!(status["satisfied"] == false, "{status}");
    Ok(())
}

// node-workspace-ac6
#[tokio::test]
async fn search_timeline_and_tasks_stay_in_their_own_workspace() -> Result<()> {
    let w = world().await?;
    w.ok(
        &w.owner,
        &w.p,
        World::save(
            "component",
            "backend",
            0,
            None,
            json!({"title":"Backend","owner":"owner","workspace":w.c}),
        ),
    )
    .await?;
    let found = w
        .mcp(
            &w.owner,
            "joppa_search",
            json!({"workspace":w.p,"query":"contract"}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    ensure!(
        found["items"].as_array().is_some_and(Vec::is_empty),
        "P search finds no C object: {found}"
    );
    let found = w
        .mcp(
            &w.owner,
            "joppa_search",
            json!({"workspace":w.c,"query":"contract"}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let kinds: Vec<&str> = found["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["id"].as_str())
        .collect();
    ensure!(
        kinds.contains(&"api"),
        "C search finds its own object: {found}"
    );
    let timeline = w
        .mcp(
            &w.owner,
            "joppa_timeline",
            json!({"workspace":w.p,"limit":100}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let addresses: Vec<&str> = timeline["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["address"].as_str())
        .collect();
    ensure!(
        !addresses
            .iter()
            .any(|a| *a == "requirement:api" || *a == "ac:x"),
        "{addresses:?}"
    );
    let tasks = w
        .mcp(
            &w.owner,
            "joppa_objects",
            json!({"workspace":w.p,"parent":"*","limit":100}),
        )
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let listed: Vec<&str> = tasks["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["address"].as_str())
        .collect();
    ensure!(
        listed.contains(&"component:backend") && !listed.contains(&"requirement:api"),
        "{listed:?}"
    );
    Ok(())
}

/// P: task T under A, claimed by bot while API is green.
async fn claimed_task(w: &World) -> Result<()> {
    w.block_a_on_api().await?;
    w.record("b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1", "pass")
        .await?;
    w.ok(&w.owner, &w.p, json!({"op":"save_object","kind":"task","id":"t","revision":0,"parent":"ac:a",
        "data":{"title":"Build login","owner":"owner","contract":"Call the API","purpose":"implementation","projects":[]},"reason":"Fixture"})).await?;
    let t = w.revision(&w.p, "task:t").await?;
    w.ok(
        &w.bot,
        &w.p,
        json!({"op":"claim_task","id":"task:t","revision":t}),
    )
    .await?;
    Ok(())
}

async fn complete(w: &World) -> Result<Result<Value, String>> {
    let t = w.revision(&w.p, "task:t").await?;
    w.command(
        &w.bot,
        &w.p,
        json!({"op":"complete_task","commit":"c0ffee0","id":"task:t","revision":t,"evidence":"Called the API"}),
    )
    .await
}

// node-workspace-ac5
#[tokio::test]
async fn completion_and_acceptance_recheck_a_remote_target_under_the_locks() -> Result<()> {
    let w = world().await?;
    claimed_task(&w).await?;
    // API turns red after the claim: the holder cannot complete until it is green again.
    w.record("b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2", "fail")
        .await?;
    match complete(&w).await? {
        Err(e) if e.contains(&w.waiting()) => {}
        other => anyhow::bail!("completion must wait for API: {other:?}"),
    }
    w.record("b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3", "pass")
        .await?;
    complete(&w)
        .await?
        .map_err(|e| anyhow::anyhow!("completion after green: {e}"))?;
    // R passes its own tests, but its acceptance waits while API is red.
    w.ok(
        &w.owner,
        &w.p,
        json!({"op":"set_ac_tests","target":"ac:a","tests":["tests/a.rs > a"],"reason":"Bind"}),
    )
    .await?;
    let commit = "c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1";
    w.mcp(&w.bot, "joppa_record_results", json!({"workspace":w.p,"commit":commit,"results":[{"test":"tests/a.rs > a","status":"pass"}]}))
        .await?.map_err(|e| anyhow::anyhow!("{e}"))?;
    w.record("b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4", "fail")
        .await?;
    let accept = async || {
        let r = w.revision(&w.p, "requirement:r").await?;
        let req = w.object(&w.owner, &w.p, "ac:a").await?["process"]["revision"].clone();
        w.command(
            &w.owner,
            &w.p,
            json!({"op":"accept_commit","target":"requirement:r","revision":req,"commit":commit}),
        )
        .await
        .map(|reply| (r, reply))
    };
    match accept().await?.1 {
        Err(e) if e.contains(&w.waiting()) => {}
        other => anyhow::bail!("acceptance must wait for API: {other:?}"),
    }
    w.record("b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5", "pass")
        .await?;
    accept()
        .await?
        .1
        .map_err(|e| anyhow::anyhow!("acceptance after green: {e}"))?;
    Ok(())
}

// node-workspace-ac5
#[tokio::test]
async fn a_parallel_completion_and_failing_record_finish_in_one_consistent_order() -> Result<()> {
    let w = world().await?;
    claimed_task(&w).await?;
    let (done, record) = tokio::join!(
        complete(&w),
        w.record("b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6", "fail")
    );
    record?;
    let done = done?;
    ensure!(
        done.is_ok() || done.as_ref().is_err_and(|e| e.contains(&w.waiting())),
        "the completion either ran first or saw the red target: {done:?}"
    );
    Ok(())
}

/// Q is a third workspace: P:A depends on C:API, and C:API depends on Q:CORE. Both green.
async fn chain_through_q(w: &World) -> Result<String> {
    let q = workspace(&w.pool, "core", false).await?;
    w.ok(
        &w.owner,
        &q,
        World::save(
            "requirement",
            "core",
            0,
            None,
            json!({"title":"Core","owner":"owner","body":"The core rule"}),
        ),
    )
    .await?;
    w.ok(
        &w.owner,
        &q,
        World::save(
            "ac",
            "y",
            0,
            Some("requirement:core"),
            json!({"title":"Y","owner":"owner"}),
        ),
    )
    .await?;
    w.ok(
        &w.owner,
        &q,
        json!({"op":"set_ac_tests","target":"ac:y","tests":[TEST],"reason":"Bind"}),
    )
    .await?;
    let core = w.revision(&q, "requirement:core").await?;
    let to_core = json!([{"workspace":q,"target":"requirement:core","relation":"depends_on","revision":core,"blocking":true}]);
    w.set_links(&w.owner, &w.c, "requirement:api", to_core)
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    w.block_a_on_api().await?;
    w.record_in(&w.c, "d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1", "pass")
        .await?;
    w.record_in(&q, "d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2", "pass")
        .await?;
    Ok(q)
}

// node-workspace-ac5
#[tokio::test]
async fn a_claim_locks_every_workspace_of_a_remote_chain() -> Result<()> {
    let w = world().await?;
    let q = chain_through_q(&w).await?;
    // A transaction holds the lock of Q, the second hop, as a record there does.
    let mut conn = w.pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&q],
    )
    .await?;
    let started = std::time::Instant::now();
    let claim = async {
        let result = w.claim().await;
        (result, started.elapsed())
    };
    let release = async {
        tokio::time::sleep(Duration::from_millis(600)).await;
        tx.commit().await
    };
    let ((claim, waited), released) = tokio::join!(claim, release);
    released?;
    claim?.map_err(|e| anyhow::anyhow!("claim after the lock: {e}"))?;
    ensure!(
        waited >= Duration::from_millis(550),
        "the claim must wait for the lock of Q: {waited:?}"
    );
    Ok(())
}

// node-workspace-ac5
#[tokio::test]
async fn a_parallel_claim_and_failing_record_two_hops_away_finish_in_one_order() -> Result<()> {
    let w = world().await?;
    let q = chain_through_q(&w).await?;
    let (claim, record) = tokio::join!(
        w.claim(),
        w.record_in(&q, "d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3", "fail")
    );
    record?;
    let claim = claim?;
    ensure!(
        claim.is_ok() || claim.as_ref().is_err_and(|e| e.contains(&w.waiting())),
        "the claim either ran first or saw the red second hop: {claim:?}"
    );
    let status = w.object(&w.bot, &w.p, "ac:a").await?["remote_links"][0]["status"].clone();
    ensure!(
        status["satisfied"] == false,
        "after both, CORE is red: {status}"
    );
    Ok(())
}

/// C publishes a new revision of API, X is green on it, and A repins to it.
async fn repin_api(w: &World) -> Result<()> {
    let pin = w.revision(&w.c, "requirement:api").await?;
    w.ok(
        &w.owner,
        &w.c,
        World::save(
            "requirement",
            "api",
            pin,
            None,
            json!({"title":"API","owner":"owner","body":"The API contract, version 2"}),
        ),
    )
    .await?;
    w.record("e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1", "pass")
        .await?;
    let pin = w.revision(&w.c, "requirement:api").await?;
    w.set_links(&w.bot, &w.p, "ac:a", json!([w.link(Some(pin), true)]))
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

// node-workspace-ac3
#[tokio::test]
async fn a_repin_after_a_claim_stops_the_old_plan_and_the_old_task() -> Result<()> {
    let w = world().await?;
    claimed_task(&w).await?;
    // Planning of A started on the old pin.
    w.claim()
        .await?
        .map_err(|e| anyhow::anyhow!("planning claim: {e}"))?;
    repin_api(&w).await?;
    let changed = "the remote links changed after the claim; release and claim again";
    match w
        .command(
            &w.bot,
            &w.p,
            json!({"op":"finish_planning","ac":"ac:a","reason":"The old plan"}),
        )
        .await?
    {
        Err(e) if e.contains(changed) => {}
        other => anyhow::bail!("the old plan must not finish: {other:?}"),
    }
    match complete(&w).await? {
        Err(e) if e.contains(changed) => {}
        other => anyhow::bail!("the old task must not complete: {other:?}"),
    }
    // A new claim after the review of the new pin can complete.
    let t = w.revision(&w.p, "task:t").await?;
    w.ok(
        &w.bot,
        &w.p,
        json!({"op":"release_task","id":"task:t","revision":t,"reason":"The pin changed"}),
    )
    .await?;
    let a = w.object(&w.owner, &w.p, "ac:a").await?;
    w.ok(&w.owner, &w.p, json!({"op":"assess","req":a["process"]["req"],"revision":a["process"]["revision"],"ac":a["process"]["criterion"],
        "assessment":{"ready":true,"reason":"The new contract is reviewed"}})).await?;
    let t = w.revision(&w.p, "task:t").await?;
    w.ok(
        &w.bot,
        &w.p,
        json!({"op":"claim_task","id":"task:t","revision":t}),
    )
    .await?;
    complete(&w)
        .await?
        .map_err(|e| anyhow::anyhow!("completion after a new claim: {e}"))?;
    Ok(())
}
