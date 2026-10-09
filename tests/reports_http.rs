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
    // missing-is-not-pass-ac2: real bun 1.4.2 output of test.each with one name, one case fails.
    let w = world().await?;
    let same = "tests/auth/login.test.ts > login > passkey > same name";
    w.list(&[same]).await?;
    let bun = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="bun test" tests="2" assertions="2" failures="1" skipped="0" time="0.04">
  <testsuite name="tests/auth/login.test.ts" file="tests/auth/login.test.ts" tests="2">
    <testsuite name="login" file="tests/auth/login.test.ts" line="2" tests="2">
      <testsuite name="passkey" file="tests/auth/login.test.ts" line="3" tests="2">
        <testcase name="same name" classname="passkey &gt; login" time="0.000075" file="tests/auth/login.test.ts" line="5" assertions="1" />
        <testcase name="same name" classname="passkey &gt; login" time="0.000794" file="tests/auth/login.test.ts" line="5" assertions="1">
          <failure type="AssertionError" message="Expected: 3&#10;Received: 2&#10;">AssertionError</failure>
        </testcase>
      </testsuite>
    </testsuite>
  </testsuite>
</testsuites>"#;
    w.upload("c1c1c1c", 1, true, bun.into()).await?;
    let s = w.status().await?;
    ensure!(
        s["status"] == "red" && s["tests"][0]["status"] == "fail",
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

fn stamp(commit: &str, order: i64, dirty: bool) -> Value {
    json!({"commit":commit,"order":order,"branch":"master","default_branch":true,"dirty":dirty,"full":true})
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn every_report_records_its_commit_and_uploader() -> Result<()> {
    // no-manual-pass-ac1: the agent token uploads; the stored report names the commit and the agent.
    let w = world().await?;
    let (code, reply) = w
        .raw_upload(
            Some(&w.token),
            stamp("c1c1c1c", 1, false),
            junit(&[("opens", "pass")]),
        )
        .await?;
    ensure!(
        code == 200
            && reply["commit"] == "c1c1c1c"
            && reply["uploader"] == "agent"
            && reply["report"].is_i64(),
        "{reply}"
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
        code == 401 && reply["error"] == "Please sign in to continue",
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
        code == 401 && reply["error"] == "Please sign in to continue",
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
        s["tests_revision"] == 2 && s["tests"].as_array().map(Vec::len) == Some(1),
        "{s}"
    );
    Ok(())
}
