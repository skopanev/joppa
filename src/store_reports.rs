//! JUnit report upload and the test status of one criterion.
use super::*;
use crate::ac_tests::{AcTests, Report, TestResult, criterion_status, parse_junit, text_hash};
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const REPORT_MAX: usize = 5 * 1024 * 1024;
/// A stored failure message keeps its first 8 KB; the full log stays in the raw report.
const MESSAGE_MAX: usize = 8 * 1024;

/// What the test run knew about itself, written by the project's preload and hook.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stamp {
    pub commit: String,
    #[serde(default)]
    pub commit_time: Option<String>,
    /// First-parent position on the default branch (`git rev-list --count --first-parent`).
    pub order: i64,
    pub branch: String,
    pub default_branch: bool,
    pub dirty: bool,
    pub full: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportUpload {
    pub stamp: Stamp,
    pub junit: String,
}

pub async fn upload_report(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    upload: ReportUpload,
) -> Result<Value> {
    let stamp = &upload.stamp;
    ensure!(upload.junit.len() <= REPORT_MAX, "a report is at most 5 MB");
    ensure!(
        (7..=64).contains(&stamp.commit.len())
            && stamp
                .commit
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "stamp.commit must be a lowercase git sha"
    );
    ensure!(!stamp.branch.trim().is_empty(), "stamp.branch is required");
    let mut results = parse_junit(&upload.junit)?;
    for result in &mut results {
        if let Some(message) = result.message.as_mut().filter(|m| m.len() > MESSAGE_MAX) {
            let mut end = MESSAGE_MAX;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
    }
    let stamp_hash = format!(
        "{:x}",
        Sha256::digest(format!("{}\n{}", serde_json::to_string(stamp)?, upload.junit).as_bytes())
    );
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let workspace = resolve_workspace(&*tx, workspace).await?;
    access::check(&*tx, &workspace, actor, false).await?;
    let inserted = tx.query_opt(
        "INSERT INTO joppa.test_reports(workspace,commit_sha,commit_time,commit_order,branch,default_branch,dirty,full_run,uploader,stamp_hash,junit) VALUES($1,$2,$3::text::timestamptz,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT (workspace,commit_sha,stamp_hash) DO NOTHING RETURNING id",
        &[&workspace,&stamp.commit,&stamp.commit_time,&stamp.order,&stamp.branch,&stamp.default_branch,&stamp.dirty,&stamp.full,&actor,&stamp_hash,&upload.junit],
    ).await?;
    let Some(row) = inserted else {
        let id: i64 = tx.query_one("SELECT id FROM joppa.test_reports WHERE workspace=$1 AND commit_sha=$2 AND stamp_hash=$3", &[&workspace,&stamp.commit,&stamp_hash]).await?.get(0);
        return Ok(json!({"report": id, "tests": results.len(), "replayed": true}));
    };
    let id: i64 = row.get(0);
    tx.execute(
        "INSERT INTO joppa.test_results(report_id,address,status,duration_ms,message) SELECT $1,x.address,x.status,x.duration_ms,x.message FROM jsonb_to_recordset($2::text::jsonb) x(address text,status text,duration_ms bigint,message text)",
        &[&id, &serde_json::to_string(&results)?],
    ).await?;
    tx.commit().await?;
    Ok(
        json!({"report": id, "commit": stamp.commit, "tests": results.len(), "replayed": false, "uploader": actor}),
    )
}

/// Green and red criteria with their ancestors, so each row can show "green / total" below it.
pub async fn tests_overview(pool: &Pool, workspace: &str, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    let listed: Vec<String> = conn
        .query(
            "SELECT key FROM joppa.state_entries WHERE workspace=$1 AND section='ac_tests'",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let parents: std::collections::HashMap<String, Option<String>> = conn
        .query(
            "SELECT address,parent FROM joppa.objects WHERE workspace=$1 AND present",
            &[&workspace],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    drop(conn);
    let mut items = vec![];
    // Archived criteria are out of the spec; they count nowhere.
    for address in listed.into_iter().filter(|a| parents.contains_key(a)) {
        let status = criterion_tests(pool, &workspace, actor, &address).await?;
        if !["green", "red"].contains(&status["status"].as_str().unwrap_or_default()) {
            continue;
        }
        let mut ancestors = vec![];
        let mut next = parents.get(&address).cloned().flatten();
        while let Some(parent) = next.filter(|p| !ancestors.contains(p)) {
            next = parents.get(&parent).cloned().flatten();
            ancestors.push(parent);
        }
        items.push(json!({"address": address, "status": status["status"], "ancestors": ancestors}));
    }
    Ok(json!({"items": items}))
}

/// A task has no test status of its own. Only complete_task sets done; verification
/// follows the status of its criterion.
pub async fn task_status(pool: &Pool, workspace: &str, actor: &str, task: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    let row = conn
        .query_opt(
            "SELECT o.parent,COALESCE(jsonb_typeof(o.process->'done'),'null')='string' FROM joppa.objects o JOIN joppa.board_cache b ON b.workspace=o.workspace WHERE o.workspace=$1 AND o.address=$2 AND b.first_view->'object_types'->o.kind->>'role'='task'",
            &[&workspace, &task],
        )
        .await?;
    let (criterion, done): (String, bool) = row
        .and_then(|row| Some((row.get::<_, Option<String>>(0)?, row.get(1))))
        .context("unknown task")?;
    drop(conn);
    let status = criterion_tests(pool, &workspace, actor, &criterion).await?;
    // Two independent values: done is the complete_task evidence in the Task process,
    // verification is the criterion status. A green criterion does not complete a Task.
    let verification = match status["status"].as_str() {
        Some("green") => "passed",
        Some("red") => "failed",
        _ => "unverified",
    };
    Ok(json!({
        "address": task,
        "verification": verification,
        "done": done,
        "criterion": {"address": criterion, "status": status["status"], "commit": status["commit"]},
    }))
}

/// Status of one criterion from its saved test list and all clean default-branch reports.
pub async fn criterion_tests(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    target: &str,
) -> Result<Value> {
    let conn = pool.get().await?;
    let workspace = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &workspace, actor, false).await?;
    criterion_test_statuses(&**conn, &workspace, &[target.to_owned()])
        .await?
        .remove(target)
        .context("unknown criterion")
}

/// Read test evidence once for the criteria in the requested tree branches.
pub(super) async fn criterion_test_statuses(
    client: &impl GenericClient,
    workspace: &str,
    targets: &[String],
) -> Result<std::collections::BTreeMap<String, Value>> {
    let rows = client.query("SELECT o.address,o.title,o.description,a.value::text FROM joppa.objects o LEFT JOIN joppa.state_entries a ON a.workspace=o.workspace AND a.section='ac_tests' AND a.key=o.address WHERE o.workspace=$1 AND o.address=ANY($2)", &[&workspace,&targets]).await?;
    let mut criteria = vec![];
    for row in rows {
        let saved: Option<AcTests> = row
            .get::<_, Option<String>>(3)
            .map(|s| serde_json::from_str(&s))
            .transpose()?;
        let data =
            json!({"title":row.get::<_,String>(1),"description":row.get::<_,Option<String>>(2)});
        criteria.push((row.get::<_, String>(0), data, saved));
    }
    statuses_for_criteria(client, workspace, criteria).await
}

/// Use the current transaction's text and test list, including earlier batch writes.
pub(super) async fn planning_criterion_status(
    client: &impl GenericClient,
    workspace: &str,
    state: &crate::model::Workspace,
    target: &str,
) -> Result<Value> {
    let data = state.objects[target].current().data.clone();
    statuses_for_criteria(
        client,
        workspace,
        vec![(target.to_owned(), data, state.ac_tests.get(target).cloned())],
    )
    .await?
    .remove(target)
    .context("unknown criterion")
}

async fn statuses_for_criteria(
    client: &impl GenericClient,
    workspace: &str,
    criteria: Vec<(String, Value, Option<AcTests>)>,
) -> Result<std::collections::BTreeMap<String, Value>> {
    if criteria.is_empty() {
        return Ok(Default::default());
    }
    let tests: std::collections::BTreeSet<String> = criteria
        .iter()
        .filter_map(|(_, _, saved)| saved.as_ref().and_then(AcTests::current))
        .flat_map(|current| current.tests.iter().cloned())
        .collect();
    let reports: Vec<Report> = client
        .query("SELECT id,commit_sha,commit_order,full_run,(extract(epoch FROM received_at)*1000)::bigint FROM joppa.test_reports WHERE workspace=$1 AND default_branch AND NOT dirty", &[&workspace])
        .await?.iter().map(|r| Report { id:r.get(0),commit:r.get(1),order:r.get(2),full:r.get(3),received_ms:r.get(4) }).collect();
    let tests: Vec<String> = tests.into_iter().collect();
    let results: Vec<(i64,TestResult)> = client
        .query("SELECT t.report_id,t.address,t.status,t.duration_ms,t.message FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1 AND r.default_branch AND NOT r.dirty AND t.address=ANY($2)", &[&workspace,&tests])
        .await?.iter().map(|r| (r.get(0),TestResult { address:r.get(1),status:r.get(2),duration_ms:r.get(3),message:r.get(4) })).collect();
    let mut statuses = std::collections::BTreeMap::new();
    for (address, data, saved) in criteria {
        let current = saved.as_ref().and_then(AcTests::current);
        let tests = current.map(|r| r.tests.as_slice()).unwrap_or_default();
        let draft = current.is_some_and(|r| r.text_hash != text_hash(&data));
        let mut status = serde_json::to_value(criterion_status(tests, draft, &reports, &results))?;
        status["address"] = json!(address);
        status["tests_revision"] = json!(current.map(|r| r.revision));
        status["tests_saved_by"] = json!(current.map(|r| &r.actor));
        statuses.insert(address, status);
    }
    Ok(statuses)
}

/// An acceptance needs a clean full run on a default-branch commit where every listed
/// test of every present criterion of the requirement passed. A criterion whose text
/// changed after its test list was saved has a draft list, so it blocks acceptance.
pub async fn check_acceptance(
    client: &impl GenericClient,
    workspace: &str,
    state: &crate::model::Workspace,
    target: &str,
    commit: &str,
) -> Result<()> {
    const NEEDS: &str = "acceptance needs a full run on the commit where every listed test passes";
    let key = crate::objects::resolve_address(state, target);
    let mut tests = vec![];
    let criteria = crate::acceptance::criteria(state, key);
    ensure!(!criteria.is_empty(), NEEDS);
    for criterion in criteria {
        let listed = state.ac_tests.get(&criterion).and_then(AcTests::current);
        let data = &state.objects[&criterion].current().data;
        let list = listed
            .filter(|l| !l.tests.is_empty() && l.text_hash == text_hash(data))
            .with_context(|| NEEDS)?;
        tests.extend(list.tests.iter().cloned());
    }
    let reports = client
        .query(
            "SELECT id,default_branch,full_run,dirty FROM joppa.test_reports WHERE workspace=$1 AND commit_sha=$2",
            &[&workspace, &commit],
        )
        .await?;
    ensure!(
        reports.is_empty() || reports.iter().any(|r| r.get::<_, bool>(1)),
        "acceptance needs a default-branch commit"
    );
    for report in reports
        .iter()
        .filter(|r| r.get::<_, bool>(1) && r.get::<_, bool>(2) && !r.get::<_, bool>(3))
    {
        let id: i64 = report.get(0);
        let rows = client
            .query(
                "SELECT address,status FROM joppa.test_results WHERE report_id=$1 AND address=ANY($2)",
                &[&id, &tests],
            )
            .await?;
        // Every listed test ran, and every case of it passed.
        let passed = tests.iter().all(|test| {
            let cases: Vec<&str> = rows
                .iter()
                .filter(|r| r.get::<_, &str>(0) == test)
                .map(|r| r.get(1))
                .collect();
            !cases.is_empty() && cases.iter().all(|s| *s == "pass")
        });
        if passed {
            return Ok(());
        }
    }
    anyhow::bail!(NEEDS)
}
