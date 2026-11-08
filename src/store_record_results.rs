//! Record test outcomes for one commit in one MCP call. It is the only way to write results.
//!
//! The member supplies the commit and the results. Joppa supplies the actor from the
//! session and the time from its server clock: a call that supplies actor or at is
//! refused before any write. Each record is newer than every earlier report, so the
//! latest record of a test decides it. A record names only the tests it ran: an
//! omitted test gets no result, and a skip or todo in a record is never a pass.
//! The block test_results of the tree schema gives the status names, the fields of a
//! result and the largest record, see result_shape. The reply names in unbound the tests
//! of the record that no present criterion lists: each test must have a criterion.
use super::*;
use crate::ac_tests::TestResult;
use crate::result_shape::ResultShape;
use serde::Deserialize;

/// The ids of the present objects that each ref field of the block can name.
type Refs = std::collections::BTreeMap<String, std::collections::BTreeSet<String>>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    commit: String,
    results: Vec<Outcome>,
}

#[derive(Debug, Deserialize)]
struct Outcome {
    test: String,
    status: String,
    /// The fields of the result. The block test_results decides which are permitted.
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

/// Check the input against the test rules and turn it into stored results. Nothing is
/// written here.
fn parse(args: &Value, shape: &ResultShape, refs: &Refs) -> Result<(String, Vec<TestResult>)> {
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
    ensure!(
        !record.results.is_empty(),
        "results must name at least one test"
    );
    ensure!(
        record.results.len() <= shape.max_results,
        "a record has at most {} results",
        shape.max_results
    );
    let mut results = Vec::with_capacity(record.results.len());
    for outcome in record.results {
        ensure!(!outcome.test.trim().is_empty(), "each result needs a test");
        let checked = crate::result_check::check(shape, &outcome.status, outcome.extra, refs)
            .map_err(|e| anyhow::anyhow!("{}: {e}", outcome.test))?;
        results.push(TestResult {
            address: outcome.test,
            status: checked.status.into(),
            duration_ms: None,
            message: None,
            status_name: checked.status_name,
            fields: checked.fields,
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
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let workspace = resolve_workspace(&*tx, workspace).await?;
    access::check(&*tx, &workspace, actor, false).await?;
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&workspace],
    )
    .await?;
    // The latest schema revision gives the test rules.
    let shape: ResultShape = tx
        .query_opt("SELECT (value->'test_results')::text FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' ORDER BY revision DESC LIMIT 1", &[&workspace])
        .await?
        .and_then(|row| row.get::<_, Option<String>>(0))
        .map(|text| serde_json::from_str(&text))
        .transpose()?
        .context("the tree schema has no test_results block")?;
    // The present objects that each ref field can name.
    let mut refs = Refs::new();
    for field in &shape.fields {
        if let Some(target) = &field.target {
            let ids = tx
                .query(
                    "SELECT local_id FROM joppa.objects WHERE workspace=$1 AND kind=$2 AND present",
                    &[&workspace, target],
                )
                .await?
                .iter()
                .map(|row| row.get(0))
                .collect();
            refs.insert(field.name.clone(), ids);
        }
    }
    let (commit, results) = parse(args, &shape, &refs)?;
    // The tests of the record that have no present test node. A full run must give none.
    let names: Vec<&str> = results.iter().map(|r| r.address.as_str()).collect();
    let unbound: Vec<String> = tx
        .query(
            "WITH s AS (SELECT value FROM joppa.state_revisions WHERE workspace=$1 AND section='tree_schema' AND key='' ORDER BY revision DESC LIMIT 1),
                k AS (SELECT t.key FROM s,jsonb_each(s.value->'types') t WHERE t.value->>'role'='test')
            SELECT DISTINCT n FROM unnest($2::text[]) n WHERE NOT EXISTS (SELECT 1 FROM joppa.objects o
                WHERE o.workspace=$1 AND o.present AND o.kind IN (SELECT key FROM k) AND o.title=n) ORDER BY n",
            &[&workspace, &names],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    let body = serde_json::to_string(&results)?;
    let hash = crate::digest_hex::sha256_hex(format!("record\n{commit}\n{body}").as_bytes());
    let event = |report: i64, repeat: bool| {
        Command::RecordReport(crate::commands::RecordReportOp {
            report,
            commit: commit.clone(),
            branch: "record".into(),
            tests: results.len() as u64,
            repeat,
        })
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
        let position =
            super::record_report::record_report(&*tx, &workspace, actor, event(id, true)).await?;
        tx.commit().await?;
        return Ok(
            json!({"record": id, "commit": commit, "tests": results.len(), "replayed": true, "actor": actor, "position": position, "unbound": unbound}),
        );
    };
    let id: i64 = row.get(0);
    tx.execute(
        "INSERT INTO joppa.test_results(report_id,address,status,duration_ms,message,status_name,fields) SELECT $1,x.address,x.status,x.duration_ms,x.message,x.status_name,x.fields FROM jsonb_to_recordset($2::text::jsonb) x(address text,status text,duration_ms bigint,message text,status_name text,fields jsonb)",
        &[&id, &body],
    ).await?;
    let position =
        super::record_report::record_report(&*tx, &workspace, actor, event(id, false)).await?;
    tx.commit().await?;
    Ok(
        json!({"record": id, "commit": commit, "tests": results.len(), "replayed": false, "actor": actor, "position": position, "unbound": unbound}),
    )
}

#[cfg(test)]
#[path = "store_record_results_tests.rs"]
mod tests;
