//! Record test outcomes for one commit in one MCP call, without JUnit or a stamp.
//!
//! The member supplies the commit and the results. Joppa supplies the actor from the
//! session and the time from its server clock: a call that supplies actor or at is
//! refused before any write. Each record is newer than every earlier report, so the
//! latest record of a test decides it. A record names only the tests it ran: an
//! omitted test gets no result, and a skip or todo in a record is never a pass.
use super::*;
use crate::ac_tests::TestResult;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The largest number of results in one record.
pub const RECORD_MAX: usize = 5000;
/// A stored failure message keeps its first 8 KB.
const MESSAGE_MAX: usize = 8 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    commit: String,
    results: Vec<Outcome>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Outcome {
    test: String,
    status: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    duration_ms: Option<i64>,
}

/// Check the input and turn it into stored results. Nothing is written here.
fn parse(args: &Value) -> Result<(String, Vec<TestResult>)> {
    let mut input = args.clone();
    if let Some(map) = input.as_object_mut() {
        map.remove("workspace");
        for field in ["actor", "at"] {
            ensure!(
                !map.contains_key(field),
                "{field} comes from the server; do not supply it"
            );
        }
    }
    let record: Record = serde_json::from_value(input)
        .map_err(|e| anyhow::anyhow!("invalid record_results input: {e}"))?;
    ensure!(
        (7..=64).contains(&record.commit.len())
            && record
                .commit
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "commit must be a lowercase git sha"
    );
    ensure!(!record.results.is_empty(), "results must name at least one test");
    ensure!(
        record.results.len() <= RECORD_MAX,
        "a record has at most {RECORD_MAX} results"
    );
    let mut results = Vec::with_capacity(record.results.len());
    for outcome in record.results {
        ensure!(!outcome.test.trim().is_empty(), "each result needs a test");
        let status = match outcome.status.as_str() {
            "pass" => "pass",
            "fail" => "fail",
            "skip" => "skipped",
            "todo" => "todo",
            _ => anyhow::bail!("status must be pass, fail, skip or todo"),
        };
        let mut message = outcome.message.filter(|m| !m.is_empty());
        if let Some(text) = message.as_mut().filter(|m| m.len() > MESSAGE_MAX) {
            let mut end = MESSAGE_MAX;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        results.push(TestResult {
            address: outcome.test,
            status: status.into(),
            duration_ms: outcome.duration_ms,
            message,
        });
    }
    Ok((record.commit, results))
}

/// Store one record of test outcomes and append its journal event.
pub async fn record_results(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    args: &Value,
) -> Result<Value> {
    let (commit, results) = parse(args)?;
    let body = serde_json::to_string(&results)?;
    let hash = format!(
        "{:x}",
        Sha256::digest(format!("record\n{commit}\n{body}").as_bytes())
    );
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let workspace = resolve_workspace(&*tx, workspace).await?;
    access::check(&*tx, &workspace, actor, false).await?;
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&workspace],
    )
    .await?;
    let event = |report: i64, repeat: bool| Command::RecordReport {
        report,
        commit: commit.clone(),
        branch: "record".into(),
        tests: results.len() as u64,
        repeat,
    };
    // Each new record is newer than every stored report of the workspace.
    let inserted = tx.query_opt(
        "INSERT INTO joppa.test_reports(workspace,commit_sha,commit_time,commit_order,branch,default_branch,dirty,full_run,uploader,stamp_hash,junit,recorded) SELECT $1,$2,NULL,COALESCE(MAX(commit_order),0)+1,'record',true,false,false,$3,$4,'',true FROM joppa.test_reports WHERE workspace=$1 ON CONFLICT (workspace,commit_sha,stamp_hash) DO NOTHING RETURNING id",
        &[&workspace, &commit, &actor, &hash],
    ).await?;
    let Some(row) = inserted else {
        let id: i64 = tx.query_one("SELECT id FROM joppa.test_reports WHERE workspace=$1 AND commit_sha=$2 AND stamp_hash=$3", &[&workspace,&commit,&hash]).await?.get(0);
        // An identical repeat is a new receipt of the same record.
        tx.execute(
            "INSERT INTO joppa.test_report_receipts(report_id) VALUES($1)",
            &[&id],
        )
        .await?;
        let position = super::timeline::record_report(&*tx, &workspace, actor, event(id, true)).await?;
        tx.commit().await?;
        return Ok(json!({"record": id, "commit": commit, "tests": results.len(), "replayed": true, "actor": actor, "position": position}));
    };
    let id: i64 = row.get(0);
    tx.execute(
        "INSERT INTO joppa.test_results(report_id,address,status,duration_ms,message) SELECT $1,x.address,x.status,x.duration_ms,x.message FROM jsonb_to_recordset($2::text::jsonb) x(address text,status text,duration_ms bigint,message text)",
        &[&id, &body],
    ).await?;
    let position =
        super::timeline::record_report(&*tx, &workspace, actor, event(id, false)).await?;
    tx.commit().await?;
    Ok(json!({"record": id, "commit": commit, "tests": results.len(), "replayed": false, "actor": actor, "position": position}))
}

#[cfg(test)]
mod tests {
    use super::parse;
    use serde_json::json;

    #[test]
    fn a_record_maps_skip_and_keeps_the_failure_message() {
        let (commit, rows) = parse(&json!({"workspace":"w","commit":"abcdef1","results":[
            {"test":"a","status":"pass"},{"test":"b","status":"fail","message":"expected 2, got 3"},
            {"test":"c","status":"skip"},{"test":"d","status":"todo","duration_ms":4}]}))
        .unwrap();
        assert_eq!(commit, "abcdef1");
        let statuses: Vec<_> = rows.iter().map(|r| r.status.as_str()).collect();
        assert_eq!(statuses, ["pass", "fail", "skipped", "todo"]);
        assert_eq!(rows[1].message.as_deref(), Some("expected 2, got 3"));
    }

    #[test]
    fn a_record_refuses_actor_at_unknown_fields_and_bad_input() {
        for (input, error) in [
            (json!({"commit":"abcdef1","results":[{"test":"a","status":"pass"}],"actor":"x"}), "actor comes from the server"),
            (json!({"commit":"abcdef1","results":[{"test":"a","status":"pass"}],"at":"2020-01-01T00:00:00Z"}), "at comes from the server"),
            (json!({"commit":"abcdef1","results":[{"test":"a","status":"pass","actor":"x"}]}), "invalid record_results input"),
            (json!({"commit":"ABCDEF1","results":[{"test":"a","status":"pass"}]}), "lowercase git sha"),
            (json!({"commit":"abcdef1","results":[]}), "at least one test"),
            (json!({"commit":"abcdef1","results":[{"test":"a","status":"passed"}]}), "pass, fail, skip or todo"),
        ] {
            let message = parse(&input).unwrap_err().to_string();
            assert!(message.contains(error), "{input}: {message}");
        }
    }
}
