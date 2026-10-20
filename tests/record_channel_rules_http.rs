//! Rules of the record channel over a real server: only a member records, Joppa sets the
//! actor and the time, and a save of a criterion never records a test outcome.
use anyhow::{Result, ensure};
use serde_json::{Value, json};

#[path = "common/record_world.rs"]
mod record_world;
use record_world::{A, World, test, world};

const P: &str = "tests/rules.rs > p";
const F: &str = "tests/rules.rs > f";
const S: &str = "tests/rules.rs > s";
const T: &str = "tests/rules.rs > t";
const O: &str = "tests/rules.rs > o";
const X: &str = "tests/rules.rs > x";
const B: &str = "b0b0b0b";
const C: &str = "c0c0c0c";

/// The stored outcomes of one commit as (test, status, message), in test order.
async fn outcomes(w: &World, commit: &str) -> Result<Vec<(String, String, Option<String>)>> {
    let conn = w.pool.get().await?;
    let rows = conn.query("SELECT t.address,t.status,t.message FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1 AND r.commit_sha=$2 ORDER BY t.address,t.status", &[&w.ws, &commit]).await?;
    Ok(rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect())
}

/// The number of stored reports and records of the workspace.
async fn reports(w: &World) -> Result<i64> {
    let conn = w.pool.get().await?;
    let sql = "SELECT count(*) FROM joppa.test_reports WHERE workspace=$1";
    Ok(conn.query_one(sql, &[&w.ws]).await?.get(0))
}

fn row(test: &str, status: &str, message: Option<&str>) -> (String, String, Option<String>) {
    (test.into(), status.into(), message.map(Into::into))
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn one_record_stores_pass_fail_skip_and_todo() -> Result<()> {
    // no-manual-pass-ac1: historical results exist for commit B; O has no result.
    let w = world(&[P, F, S, T, O]).await?;
    w.recorded(
        B,
        json!([{"test":P,"status":"pass"},{"test":F,"status":"pass"}]),
    )
    .await?;
    let history = outcomes(&w, B).await?;
    ensure!(
        history == [row(F, "pass", None), row(P, "pass", None)],
        "{history:?}"
    );
    let before = w.position().await?;
    let results = json!([
        {"test":P,"status":"pass"},
        {"test":F,"status":"fail","message":"expected 2, got 3"},
        {"test":S,"status":"skip"},
        {"test":T,"status":"todo"}
    ]);
    let t0 = w.clock().await?;
    let reply = w.recorded(C, results).await?;
    let t1 = w.clock().await?;
    ensure!(
        reply["actor"] == "bot" && reply["commit"] == C && reply["tests"] == 4,
        "{reply}"
    );
    // Exactly one journal entry follows the earlier position.
    let after = before.as_u64().unwrap_or_default();
    let page = w
        .mcp(&w.alex, "joppa_timeline", json!({"after":after,"limit":10}))
        .await?
        .map_err(|e| anyhow::anyhow!("joppa_timeline: {e}"))?;
    let items = page["items"].as_array().cloned().unwrap_or_default();
    ensure!(items.len() == 1, "{page}");
    let entry = &items[0];
    ensure!(
        entry["op"] == "record_report"
            && entry["actor"] == "bot"
            && entry["title"] == format!("Test report {C}")
            && entry["report"] == reply["record"]
            && entry["position"] == reply["position"],
        "{entry} {reply}"
    );
    let at = entry["at"].as_str().unwrap_or_default();
    let conn = w.pool.get().await?;
    let inside: bool = conn
        .query_one(
            "SELECT $1::text::timestamptz BETWEEN $2::text::timestamptz AND $3::text::timestamptz",
            &[&at, &t0, &t1],
        )
        .await?
        .get(0);
    ensure!(inside, "at {at} is not between {t0} and {t1}");
    // The record of the entry holds the four supplied outcomes and no outcome for O.
    let id = entry["report"].as_i64().unwrap_or_default();
    let stored = conn.query("SELECT r.commit_sha,r.uploader,t.address,t.status,t.message FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.id=$1 ORDER BY t.address", &[&id]).await?;
    let stored: Vec<(String, String, String, String, Option<String>)> = stored
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)))
        .collect();
    let expected: Vec<_> = [
        row(F, "fail", Some("expected 2, got 3")),
        row(P, "pass", None),
        row(S, "skipped", None),
        row(T, "todo", None),
    ]
    .into_iter()
    .map(|(test, status, message)| (C.to_owned(), "bot".to_owned(), test, status, message))
    .collect();
    ensure!(stored == expected, "{stored:?}");
    // Member alex reads the results through joppa_tests.
    let s = w.tests_mcp(&w.alex).await?;
    for (t, status) in [(P, "pass"), (F, "fail"), (S, "skipped"), (T, "todo")] {
        ensure!(
            test(&s, t)["status"] == status && test(&s, t)["commit"] == C,
            "{t}: {s}"
        );
    }
    ensure!(test(&s, F)["message"] == "expected 2, got 3", "{s}");
    ensure!(
        test(&s, O)["status"] == "not_run" && test(&s, O)["commit"].is_null(),
        "{s}"
    );
    ensure!(
        outcomes(&w, B).await? == history,
        "the results for B changed"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_caller_without_membership_cannot_record() -> Result<()> {
    // no-manual-pass-ac2: eve has no session; outsider is signed in, but is not a member.
    let w = world(&[X]).await?;
    let call = json!({"workspace":w.ws,"commit":C,"results":[{"test":X,"status":"pass"}]});
    let before = w.position().await?;
    let (code, reply) = w
        .mcp_raw(None, "joppa_record_results", call.clone())
        .await?;
    ensure!(
        code == 401 && reply["error"] == "unauthorized",
        "eve: {code} {reply}"
    );
    ensure!(w.position().await? == before, "eve changed the journal");
    let refusal = w.record(&w.outsider, C, call["results"].clone()).await?;
    ensure!(
        refusal == Err("Workspace not found".into()),
        "outsider: {refusal:?}"
    );
    ensure!(
        w.position().await? == before,
        "outsider changed the journal"
    );
    ensure!(reports(&w).await? == 0, "a refused call stored a record");
    let s = w.tests_mcp(&w.alex).await?;
    ensure!(test(&s, X)["status"] == "not_run", "{s}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_caller_cannot_supply_the_actor_or_time_of_a_record() -> Result<()> {
    // no-manual-pass-ac3: two attempts of bot add actor and at to a valid call.
    let w = world(&[X]).await?;
    let valid = json!({"commit":C,"results":[{"test":X,"status":"pass"}]});
    for (field, value, error) in [
        (
            "actor",
            "another-member",
            "actor comes from the server; do not supply it",
        ),
        (
            "at",
            "2020-01-01T00:00:00Z",
            "at comes from the server; do not supply it",
        ),
    ] {
        let before = w.position().await?;
        let mut call = valid.clone();
        call[field] = json!(value);
        let refusal = w.mcp(&w.bot, "joppa_record_results", call).await?;
        ensure!(refusal == Err(error.into()), "{field}: {refusal:?}");
        ensure!(
            w.position().await? == before,
            "{field}: the journal changed"
        );
    }
    ensure!(reports(&w).await? == 0, "a refused call stored a record");
    let s = w.tests_mcp(&w.bot).await?;
    ensure!(test(&s, X)["status"] == "not_run", "{s}");
    Ok(())
}

/// Bot saves A through joppa_command with one more data field. Other fields stay.
async fn save(w: &World, field: &str, value: Value) -> Result<Result<Value, String>> {
    let a = w.object(A).await?;
    let mut data = a["document"]["data"].clone();
    data[field] = value;
    let command = json!({"op":"save_object","kind":"ac","id":"a","revision":a["current_revision"],"parent":a["document"]["parent"],"data":data,"links":a["document"]["links"],"reason":"Save the criterion"});
    w.command(&w.bot, command).await
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_save_cannot_record_a_test_outcome() -> Result<()> {
    // no-manual-pass-ac4, fixture 1: a save with data.verification=passed is refused.
    let w = world(&[X]).await?;
    let revision = w.object(A).await?["current_revision"].clone();
    let refusal = save(&w, "verification", json!("passed")).await?;
    ensure!(
        refusal == Err("verification comes from test reports".into()),
        "{refusal:?}"
    );
    ensure!(
        w.object(A).await?["current_revision"] == revision,
        "the save stored a revision"
    );
    let s = w.tests_mcp(&w.bot).await?;
    ensure!(test(&s, X)["status"] == "not_run", "fixture 1: {s}");
    // Fixture 2: comment is an ordinary text field, so the save succeeds.
    let w = world(&[X]).await?;
    save(&w, "comment", json!("Test X passed"))
        .await?
        .map_err(|e| anyhow::anyhow!("fixture 2: {e}"))?;
    let a = w.object(A).await?;
    ensure!(a["document"]["data"]["comment"] == "Test X passed", "{a}");
    ensure!(reports(&w).await? == 0, "the save stored a record");
    let s = w.tests_mcp(&w.bot).await?;
    ensure!(test(&s, X)["status"] == "not_run", "fixture 2: {s}");
    Ok(())
}
