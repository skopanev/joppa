//! Criterion status from clean default-branch reports.
//!
//! Each listed test counts with its latest result. The deciding report for a test
//! is the newest one (by default-branch commit order, then by upload) that either
//! ran the test or was a full run. On that commit, red wins: a failure or an
//! absence from a full run beats a pass. A criterion is green only when every
//! listed test passes; it shows the oldest commit among its tests.
//! A filter such as `bun test -t` reports every other test as skipped, so skipped
//! and todo results count only in full runs; a partial run did not run those tests.
use super::TestResult;
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct Report {
    pub id: i64,
    pub commit: String,
    /// First-parent position on the default branch. Higher is newer.
    pub order: i64,
    pub full: bool,
    pub received_ms: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TestStatus {
    pub address: String,
    /// One of pass, fail, missing, skipped, todo, not_run.
    pub status: String,
    pub commit: Option<String>,
    pub message: Option<String>,
    #[serde(skip)]
    order: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CriterionStatus {
    /// One of green, red, grey.
    pub status: String,
    pub reason: Option<String>,
    pub tests_draft: bool,
    pub commit: Option<String>,
    pub commits_behind: Option<i64>,
    pub tests: Vec<TestStatus>,
}

fn test_status(address: &str, reports: &[Report], all: &[(i64, TestResult)]) -> TestStatus {
    let full = |id: i64| reports.iter().any(|r| r.id == id && r.full);
    let results: Vec<(i64, TestResult)> = all
        .iter()
        .filter(|(id, r)| full(*id) || !matches!(r.status.as_str(), "skipped" | "todo"))
        .cloned()
        .collect();
    let results = results.as_slice();
    let ran = |report: &Report| {
        results
            .iter()
            .any(|(id, r)| *id == report.id && r.address == address)
    };
    let mut ordered: Vec<&Report> = reports.iter().collect();
    ordered.sort_by(|a, b| {
        b.order
            .cmp(&a.order)
            .then(b.received_ms.cmp(&a.received_ms))
    });
    let Some(deciding) = ordered.into_iter().find(|r| r.full || ran(r)) else {
        return TestStatus {
            address: address.into(),
            status: "not_run".into(),
            commit: None,
            message: None,
            order: None,
        };
    };
    let same_commit: Vec<&Report> = reports
        .iter()
        .filter(|r| r.commit == deciding.commit)
        .collect();
    let rows: Vec<&TestResult> = results
        .iter()
        .filter(|(id, r)| r.address == address && same_commit.iter().any(|report| report.id == *id))
        .map(|(_, r)| r)
        .collect();
    let missing = same_commit.iter().any(|report| report.full && !ran(report));
    let failed = rows.iter().find(|r| r.status == "fail");
    let (status, message) = if let Some(failure) = failed {
        ("fail", failure.message.clone())
    } else if missing {
        ("missing", Some("the test is absent from a full run".into()))
    } else if rows.iter().any(|r| r.status == "skipped") {
        ("skipped", None)
    } else if rows.iter().any(|r| r.status == "todo") {
        ("todo", None)
    } else {
        ("pass", None)
    };
    TestStatus {
        address: address.into(),
        status: status.into(),
        commit: Some(deciding.commit.clone()),
        message,
        order: Some(deciding.order),
    }
}

pub fn criterion_status(
    tests: &[String],
    draft: bool,
    reports: &[Report],
    results: &[(i64, TestResult)],
) -> CriterionStatus {
    let statuses: Vec<TestStatus> = tests
        .iter()
        .map(|t| test_status(t, reports, results))
        .collect();
    let (status, reason) = if tests.is_empty() {
        ("grey", Some("no tests"))
    } else if draft {
        ("grey", Some("tests_draft"))
    } else if statuses
        .iter()
        .any(|t| t.status == "fail" || t.status == "missing")
    {
        ("red", None)
    } else if statuses.iter().all(|t| t.status == "pass") {
        ("green", None)
    } else {
        ("grey", Some("not all tests passed"))
    };
    let oldest = statuses
        .iter()
        .filter(|t| t.order.is_some())
        .min_by_key(|t| t.order);
    let newest_order = reports.iter().map(|r| r.order).max();
    CriterionStatus {
        status: status.into(),
        reason: reason.map(String::from),
        tests_draft: draft,
        commit: oldest.and_then(|t| t.commit.clone()),
        commits_behind: oldest.and_then(|t| Some(newest_order? - t.order?)),
        tests: statuses,
    }
}
