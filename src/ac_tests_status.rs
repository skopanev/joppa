//! Criterion status from clean default-branch reports.
//!
//! Each listed test counts with its latest result. The deciding report for a test
//! is the newest one (by default-branch commit order, then by upload) that either
//! ran the test or was a full run. On that commit, red wins: a failure or an
//! absence from a full run beats a pass. A criterion is green only when every
//! listed test passes; it shows the oldest commit among its tests.
//! A filter such as `bun test -t` reports every other test as skipped, so skipped
//! and todo results count only in full runs; a partial run did not run those tests.
//! A change above a criterion sets its boundary. Reports received at or before it do
//! not count, and after it only one clean full run where every listed test passes
//! gives green: partial runs no longer combine. An identical repeat of a report is a
//! new receipt of it, and its latest receipt decides whether it came after the change.
//! After a change, an eligible full run decides even when the list is a draft.
use super::TestResult;
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct Report {
    pub id: i64,
    pub commit: String,
    /// First-parent position on the default branch. Higher is newer.
    pub order: i64,
    pub full: bool,
    /// First receipt; it orders reports with the same commit order.
    pub received_us: i64,
    /// Latest receipt, including identical repeats; it is compared with the boundary.
    pub receipt_us: i64,
}

/// The latest change at or above a criterion, from `Workspace::criterion_boundaries`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    /// No change reset the criterion: every report counts.
    None,
    /// Only reports received after this time, in microseconds, count.
    After(i64),
    /// The stored boundary cannot be read, so no report counts.
    Unreadable,
}

impl Boundary {
    /// Read a stored journal `recorded_at` time.
    pub fn read(stored: Option<&str>) -> Self {
        let Some(at) = stored else {
            return Self::None;
        };
        chrono::DateTime::parse_from_rfc3339(at)
            .or_else(|_| chrono::DateTime::parse_from_str(at, "%Y-%m-%d %H:%M:%S%.f%#z"))
            .map_or(Self::Unreadable, |time| {
                Self::After(time.timestamp_micros())
            })
    }
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
            .then(b.received_us.cmp(&a.received_us))
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

/// One full run where every listed test ran and every case of it passed.
fn full_pass(tests: &[String], reports: &[Report], results: &[(i64, TestResult)]) -> bool {
    reports.iter().filter(|r| r.full).any(|report| {
        tests.iter().all(|test| {
            let mut cases = results
                .iter()
                .filter(|(id, r)| *id == report.id && &r.address == test)
                .peekable();
            cases.peek().is_some() && cases.all(|(_, r)| r.status == "pass")
        })
    })
}

pub fn criterion_status(
    tests: &[String],
    draft: bool,
    boundary: Boundary,
    reports: &[Report],
    results: &[(i64, TestResult)],
) -> CriterionStatus {
    let eligible: Vec<Report> = reports
        .iter()
        .filter(|r| match boundary {
            Boundary::None => true,
            Boundary::After(at) => r.receipt_us > at,
            Boundary::Unreadable => false,
        })
        .cloned()
        .collect();
    let reports = eligible.as_slice();
    let statuses: Vec<TestStatus> = tests
        .iter()
        .map(|t| test_status(t, reports, results))
        .collect();
    let decided_after_change =
        matches!(boundary, Boundary::After(_)) && reports.iter().any(|r| r.full);
    let (status, reason) = if tests.is_empty() {
        ("grey", Some("no tests"))
    } else if draft && !decided_after_change {
        ("grey", Some("tests_draft"))
    } else if boundary == Boundary::Unreadable {
        ("grey", Some("the reset time cannot be read"))
    } else if statuses
        .iter()
        .any(|t| t.status == "fail" || t.status == "missing")
    {
        ("red", None)
    } else if !statuses.iter().all(|t| t.status == "pass") {
        ("grey", Some("not all tests passed"))
    } else if boundary != Boundary::None && !full_pass(tests, reports, results) {
        (
            "grey",
            Some("a full run after the change must pass every test"),
        )
    } else {
        ("green", None)
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
