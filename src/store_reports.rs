//! JUnit report upload and the test status of one criterion.
use super::*;
use crate::ac_tests::{
    AcTests, Boundary, Report, TestResult, criterion_status, parse_junit, text_hash,
};
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

#[path = "reports_upload.rs"]
mod upload;
pub use upload::upload_report;

#[path = "reports_overview.rs"]
mod overview;
pub use overview::{criterion_tests, task_status, tests_overview};

#[path = "reports_acceptance.rs"]
mod acceptance;
pub use acceptance::check_acceptance;

/// Read test evidence once for the criteria in the requested tree branches.
/// A target in `optional` with an unreadable saved list is left out of the result.
pub(super) async fn criterion_test_statuses(
    client: &impl GenericClient,
    workspace: &str,
    targets: &[String],
    optional: &std::collections::BTreeSet<String>,
) -> Result<std::collections::BTreeMap<String, Value>> {
    let rows = client.query("SELECT o.address,o.title,o.description,a.value::text,b.value#>>'{}' FROM joppa.objects o LEFT JOIN joppa.state_entries a ON a.workspace=o.workspace AND a.section='ac_tests' AND a.key=o.address LEFT JOIN joppa.state_entries b ON b.workspace=o.workspace AND b.section='criterion_boundaries' AND b.key=o.address WHERE o.workspace=$1 AND o.address=ANY($2)", &[&workspace,&targets]).await?;
    let mut criteria = vec![];
    for row in rows {
        let saved: Option<AcTests> = match row
            .get::<_, Option<String>>(3)
            .map(|s| serde_json::from_str(&s))
            .transpose()
        {
            Ok(saved) => saved,
            Err(_) if optional.contains(row.get::<_, &str>(0)) => continue,
            Err(error) => return Err(error.into()),
        };
        let data =
            json!({"title":row.get::<_,String>(1),"description":row.get::<_,Option<String>>(2)});
        let boundary = Boundary::read(row.get::<_, Option<&str>>(4));
        criteria.push((row.get::<_, String>(0), data, saved, boundary));
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
    state_criterion_statuses(client, workspace, state, &[target.to_owned()])
        .await?
        .remove(target)
        .context("unknown criterion")
}

/// Statuses of the criteria from the current transaction's texts and test lists.
pub(super) async fn state_criterion_statuses(
    client: &impl GenericClient,
    workspace: &str,
    state: &crate::model::Workspace,
    targets: &[String],
) -> Result<std::collections::BTreeMap<String, Value>> {
    let criteria = targets
        .iter()
        .filter_map(|target| {
            let data = state.objects.get(target)?.current().data.clone();
            let boundary =
                Boundary::read(state.criterion_boundaries.get(target).map(String::as_str));
            Some((
                target.clone(),
                data,
                state.ac_tests.get(target).cloned(),
                boundary,
            ))
        })
        .collect();
    statuses_for_criteria(client, workspace, criteria).await
}

async fn statuses_for_criteria(
    client: &impl GenericClient,
    workspace: &str,
    criteria: Vec<(String, Value, Option<AcTests>, Boundary)>,
) -> Result<std::collections::BTreeMap<String, Value>> {
    if criteria.is_empty() {
        return Ok(Default::default());
    }
    let tests: std::collections::BTreeSet<String> = criteria
        .iter()
        .filter_map(|(_, _, saved, _)| saved.as_ref().and_then(AcTests::current))
        .flat_map(|current| current.tests.iter().cloned())
        .collect();
    let reports: Vec<Report> = client
        .query("SELECT r.id,r.commit_sha,r.commit_order,r.full_run,(extract(epoch FROM r.received_at)*1000000)::bigint,(extract(epoch FROM greatest(r.received_at,(SELECT max(c.received_at) FROM joppa.test_report_receipts c WHERE c.report_id=r.id)))*1000000)::bigint FROM joppa.test_reports r WHERE r.workspace=$1 AND r.default_branch AND NOT r.dirty", &[&workspace])
        .await?.iter().map(|r| Report { id:r.get(0),commit:r.get(1),order:r.get(2),full:r.get(3),received_us:r.get(4),receipt_us:r.get(5) }).collect();
    let tests: Vec<String> = tests.into_iter().collect();
    let results: Vec<(i64,TestResult)> = client
        .query("SELECT t.report_id,t.address,t.status,t.duration_ms,t.message FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1 AND r.default_branch AND NOT r.dirty AND t.address=ANY($2)", &[&workspace,&tests])
        .await?.iter().map(|r| (r.get(0),TestResult { address:r.get(1),status:r.get(2),duration_ms:r.get(3),message:r.get(4) })).collect();
    let mut statuses = std::collections::BTreeMap::new();
    for (address, data, saved, boundary) in criteria {
        let current = saved.as_ref().and_then(AcTests::current);
        let tests = current.map(|r| r.tests.as_slice()).unwrap_or_default();
        let draft = current.is_some_and(|r| r.text_hash != text_hash(&data));
        let mut status =
            serde_json::to_value(criterion_status(tests, draft, boundary, &reports, &results))?;
        status["address"] = json!(address);
        status["tests_revision"] = json!(current.map(|r| r.revision));
        status["tests_saved_by"] = json!(current.map(|r| &r.actor));
        statuses.insert(address, status);
    }
    Ok(statuses)
}
