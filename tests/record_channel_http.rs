//! Criterion status from records over a real server: member bot records outcomes through
//! the MCP tool joppa_record_results, and a member reads GET /tests?ac=A.
use anyhow::{Result, ensure};
use serde_json::{Value, json};

#[path = "common/record_world.rs"]
mod record_world;
use record_world::{World, test, world};

const X: &str = "tests/record.rs > x";
const Y: &str = "tests/record.rs > y";

fn pass(tests: &[&str]) -> Value {
    tests
        .iter()
        .map(|t| json!({"test":t,"status":"pass"}))
        .collect()
}

/// Member bot changes the title of A, so A has a new text.
async fn change_text(w: &World) -> Result<()> {
    let a = w.object(record_world::A).await?;
    let command = json!({"op":"save_object","kind":"ac","id":"a","revision":a["current_revision"],"parent":"requirement:r","data":{"title":"A changed","owner":"owner"},"reason":"Sharper wording"});
    w.command(&w.bot, command)
        .await?
        .map_err(|e| anyhow::anyhow!("text change: {e}"))?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn only_a_record_after_the_last_change_turns_a_criterion_green() -> Result<()> {
    // check-list-frozen-ac4: fixture 1 records on C1 again, fixture 2 records on C2.
    let (c1, c2) = ("c1c1c1c", "c2c2c2c");
    for (fixture, commit) in [(1, c1), (2, c2)] {
        let w = world(&[X, Y]).await?;
        w.recorded(c1, pass(&[X, Y])).await?;
        let s = w.tests_http(&w.alex).await?;
        ensure!(
            s["status"] == "green",
            "fixture {fixture}, record on C1: {s}"
        );
        change_text(&w).await?;
        let s = w.tests_http(&w.alex).await?;
        ensure!(s["status"] == "grey", "fixture {fixture}, before: {s}");
        w.recorded(commit, pass(&[X, Y])).await?;
        let s = w.tests_http(&w.alex).await?;
        ensure!(s["status"] == "green", "fixture {fixture}, after: {s}");
        for t in [X, Y] {
            ensure!(
                test(&s, t)["status"] == "pass" && test(&s, t)["commit"] == commit,
                "fixture {fixture}, {t}: {s}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn the_newest_record_decides_a_test_and_not_the_newest_commit() -> Result<()> {
    // latest-run-decides-ac1: in the history of the fixture, C2 is newer than C1.
    let w = world(&[X]).await?;
    let (c1, c2) = ("c1c1c1c", "c2c2c2c");
    w.recorded(c2, pass(&[X])).await?;
    w.recorded(c1, json!([{"test":X,"status":"fail","message":"boom"}]))
        .await?;
    let s = w.tests_http(&w.bot).await?;
    let x = test(&s, X);
    ensure!(x["status"] == "fail" && x["commit"] == c1, "{s}");
    ensure!(s["status"] == "red", "{s}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_recorded_skip_keeps_the_criterion_grey() -> Result<()> {
    // missing-is-not-pass-ac1: X passes on an older commit, then the latest record is skip on C.
    let w = world(&[X]).await?;
    w.recorded("b0b0b0b", pass(&[X])).await?;
    w.recorded("c0c0c0c", json!([{"test":X,"status":"skip"}]))
        .await?;
    let s = w.tests_http(&w.bot).await?;
    let x = test(&s, X);
    ensure!(x["status"] == "skipped" && x["commit"] == "c0c0c0c", "{s}");
    ensure!(s["status"] == "grey", "{s}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn one_failing_case_of_three_in_a_record_makes_the_test_red() -> Result<()> {
    // missing-is-not-pass-ac2: one record has three cases of X, and the second fails.
    let w = world(&[X]).await?;
    w.recorded(
        "c0c0c0c",
        json!([
            {"test":X,"status":"pass"},
            {"test":X,"status":"fail","message":"case 2 failed"},
            {"test":X,"status":"pass"}
        ]),
    )
    .await?;
    // Agent bot reads with its own token.
    let s = w.tests_http(&w.bot).await?;
    ensure!(test(&s, X)["status"] == "fail", "{s}");
    ensure!(s["status"] == "red", "{s}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_recorded_todo_keeps_the_criterion_grey() -> Result<()> {
    // missing-is-not-pass-ac3: X passes on an older commit, then the latest record is todo on C.
    let w = world(&[X]).await?;
    w.recorded("b0b0b0b", pass(&[X])).await?;
    w.recorded("c0c0c0c", json!([{"test":X,"status":"todo"}]))
        .await?;
    let s = w.tests_http(&w.bot).await?;
    let x = test(&s, X);
    ensure!(x["status"] == "todo" && x["commit"] == "c0c0c0c", "{s}");
    ensure!(s["status"] == "grey", "{s}");
    Ok(())
}

/// A commit that exists only on branch "feat" of a new local git repository.
fn feature_commit(dir: &std::path::Path) -> Result<String> {
    let git = |args: &[&str]| -> Result<String> {
        let out = std::process::Command::new("git")
            .args(["-c", "user.name=bot", "-c", "user.email=bot@example.test"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()?;
        ensure!(out.status.success(), "git {args:?}: {out:?}");
        Ok(String::from_utf8(out.stdout)?.trim().to_owned())
    };
    std::fs::create_dir_all(dir)?;
    git(&["init", "-q", "-b", "main"])?;
    git(&["commit", "-q", "--allow-empty", "-m", "main"])?;
    git(&["checkout", "-q", "-b", "feat"])?;
    git(&["commit", "-q", "--allow-empty", "-m", "feature"])?;
    let commit = git(&["rev-parse", "HEAD"])?;
    let branches = git(&["branch", "--format=%(refname:short)", "--contains", &commit])?;
    ensure!(branches == "feat", "C is on: {branches}");
    Ok(commit)
}

#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_record_on_a_feature_branch_commit_turns_the_criterion_green() -> Result<()> {
    // result-names-subject-ac1: commit C exists only on branch "feat".
    let dir = std::env::temp_dir().join(format!("joppa-feat-{}", uuid::Uuid::new_v4().simple()));
    let c = feature_commit(&dir)?;
    let _ = std::fs::remove_dir_all(&dir);
    let w = world(&[X]).await?;
    let s = w.tests_http(&w.bot).await?;
    ensure!(test(&s, X)["status"] == "not_run", "{s}");
    w.recorded(&c, pass(&[X])).await?;
    let s = w.tests_http(&w.bot).await?;
    let x = test(&s, X);
    ensure!(x["status"] == "pass" && x["commit"] == c.as_str(), "{s}");
    ensure!(s["status"] == "green", "{s}");
    Ok(())
}
