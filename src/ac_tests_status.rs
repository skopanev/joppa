//! Criterion status from recorded test results.
//!
//! Each listed test counts with its latest result. The deciding record for a test is
//! the newest one that names the test. The block test_results of the tree schema gives
//! the rules. With same_commit fail_wins, a failure in any record of the commit of that
//! record beats a pass. With latest_wins, only the deciding record counts. A criterion is
//! green when the outcome of every listed test is in green_if_all, and red when one is in
//! red_if_any. It shows the oldest commit among its tests.
//! A skip or todo counts only in a record. Older stored reports did not run those tests.
//! With reset_on_change, a change above a criterion sets its boundary. Records received at or before it do
//! not count. After it, the latest eligible result of each listed test decides, also
//! when the results come from several records. An identical repeat of a record is
//! a new receipt of it, and its latest receipt decides whether it came after the change.
//! After a change, a draft list is decided once every listed test has an eligible result.
use crate::result_shape::ResultShape;
use crate::test_rules::outcome;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TestResult {
    pub address: String,
    /// One of pass, fail, skipped, todo.
    pub status: String,
    pub duration_ms: Option<i64>,
    pub message: Option<String>,
    /// The status name of a schema with its own names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_name: Option<String>,
    /// The extra fields from the schema result shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<serde_json::Value>,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub id: i64,
    pub commit: String,
    /// Receipt order in the workspace. Higher is newer.
    pub order: i64,
    /// A record_results report: its skip and todo results count.
    pub recorded: bool,
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
    /// One of pass, fail, skipped, todo, not_run.
    pub status: String,
    pub commit: Option<String>,
    pub message: Option<String>,
    /// The status name of the deciding result, when the schema has its own names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_name: Option<String>,
    /// The extra fields of the deciding result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<serde_json::Value>,
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

fn test_status(
    address: &str,
    reports: &[Report],
    all: &[(i64, TestResult)],
    rules: &ResultShape,
) -> TestStatus {
    let counts_skips = |id: i64| reports.iter().any(|r| r.id == id && r.recorded);
    let results: Vec<(i64, TestResult)> = all
        .iter()
        .filter(|(id, r)| counts_skips(*id) || !matches!(r.status.as_str(), "skipped" | "todo"))
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
    let Some(deciding) = ordered.into_iter().find(|r| ran(r)) else {
        return TestStatus {
            address: address.into(),
            status: "not_run".into(),
            commit: None,
            message: None,
            status_name: None,
            fields: None,
            order: None,
        };
    };
    let same_commit: Vec<&Report> = reports
        .iter()
        .filter(|r| match rules.same_commit.as_str() {
            "fail_wins" => r.commit == deciding.commit,
            _ => r.id == deciding.id,
        })
        .collect();
    let rows: Vec<&TestResult> = results
        .iter()
        .filter(|(id, r)| r.address == address && same_commit.iter().any(|report| report.id == *id))
        .map(|(_, r)| r)
        .collect();
    let status = ["fail", "skipped", "todo"]
        .into_iter()
        .find(|s| rows.iter().any(|r| r.status == *s))
        .unwrap_or("pass");
    // The deciding row gives the message, the status name and the fields.
    let row = rows.iter().find(|r| r.status == status);
    TestStatus {
        address: address.into(),
        status: status.into(),
        commit: Some(deciding.commit.clone()),
        // Older rows keep the message in its own column. Newer rows keep it in the
        // failure field.
        message: row.filter(|_| status == "fail").and_then(|r| {
            r.message.clone().or_else(|| {
                let field = rules.failure_field.as_deref()?;
                let fields = r.fields.as_ref()?;
                fields.get(field)?.as_str().map(String::from)
            })
        }),
        status_name: row.and_then(|r| r.status_name.clone()),
        fields: row.and_then(|r| r.fields.clone()),
        order: Some(deciding.order),
    }
}

pub fn criterion_status(
    tests: &[String],
    draft: bool,
    boundary: Boundary,
    reports: &[Report],
    results: &[(i64, TestResult)],
    rules: &ResultShape,
) -> CriterionStatus {
    let boundary = if rules.reset_on_change {
        boundary
    } else {
        Boundary::None
    };
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
        .map(|t| test_status(t, reports, results, rules))
        .collect();
    // After a change, a draft list is decided when every listed test has an eligible result.
    let decided_after_change =
        matches!(boundary, Boundary::After(_)) && statuses.iter().all(|t| t.status != "not_run");
    let (status, reason) = if tests.is_empty() {
        ("grey", Some("no tests"))
    } else if draft && !decided_after_change {
        ("grey", Some("tests_draft"))
    } else if boundary == Boundary::Unreadable {
        ("grey", Some("the reset time cannot be read"))
    } else if statuses
        .iter()
        .any(|t| rules.red_if_any.iter().any(|o| o == outcome(&t.status)))
    {
        ("red", None)
    } else if !statuses
        .iter()
        .all(|t| rules.green_if_all.iter().any(|o| o == outcome(&t.status)))
    {
        ("grey", Some("not all tests passed"))
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
