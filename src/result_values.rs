//! The status of a criterion for each value of a board field of the result shape.
//!
//! A board field splits the results of a criterion by its value. Each value gets its own
//! status from the results with that value: the latest result of a test for that value
//! decides the test. The criterion status combines the values: red when one value is red,
//! green when every value is green, and grey otherwise. A value without results is grey.
//! A workspace without a board field keeps the status of all its results.
use crate::ac_tests::{Boundary, CriterionStatus, Report, TestResult, criterion_status};
use anyhow::Result;
use serde_json::{Map, Value};

/// Test statuses from the worst to the best. The criterion shows the worst of the values.
const WORST_FIRST: [&str; 5] = ["fail", "not_run", "skipped", "todo", "pass"];
/// The reasons of a grey status that do not depend on the results.
const OWN_REASONS: [&str; 3] = ["no tests", "tests_draft", "the reset time cannot be read"];

/// The criterion status with `values`: for each board field, the status of each value.
pub fn status_by_value(
    tests: &[String],
    draft: bool,
    boundary: Boundary,
    reports: &[Report],
    own: &[(i64, TestResult)],
    board: &[(String, Vec<String>)],
) -> Result<Value> {
    let base = criterion_status(tests, draft, boundary, reports, own);
    if board.is_empty() {
        return Ok(serde_json::to_value(base)?);
    }
    let mut values = Map::new();
    let mut all = vec![];
    for (field, list) in board {
        let mut by_value = Map::new();
        for value in list {
            let mine: Vec<(i64, TestResult)> = own
                .iter()
                .filter(|(_, r)| {
                    r.fields
                        .as_ref()
                        .and_then(|f| f.get(field))
                        .and_then(Value::as_str)
                        == Some(value.as_str())
                })
                .cloned()
                .collect();
            let status = criterion_status(tests, draft, boundary, reports, &mine);
            by_value.insert(value.clone(), serde_json::to_value(&status)?);
            all.push(status);
        }
        values.insert(field.clone(), Value::Object(by_value));
    }
    let mut status = serde_json::to_value(combine(base, &all))?;
    status["values"] = Value::Object(values);
    Ok(status)
}

/// One status from the statuses of all values.
fn combine(base: CriterionStatus, all: &[CriterionStatus]) -> CriterionStatus {
    let (status, reason) = if all.iter().any(|s| s.status == "red") {
        ("red", None)
    } else if all.iter().all(|s| s.status == "green") {
        ("green", None)
    } else {
        let own = base
            .reason
            .clone()
            .filter(|r| base.status == "grey" && OWN_REASONS.contains(&r.as_str()));
        (
            "grey",
            Some(own.unwrap_or_else(|| "not every value passed".into())),
        )
    };
    let rank = |s: &str| WORST_FIRST.iter().position(|w| *w == s).unwrap_or(0);
    // Each test shows its worst status among the values.
    let tests = (0..base.tests.len())
        .map(|i| {
            all.iter()
                .filter_map(|s| s.tests.get(i))
                .min_by_key(|t| rank(&t.status))
                .cloned()
                .unwrap_or_else(|| base.tests[i].clone())
        })
        .collect();
    // The oldest commit among the values is the commit of the criterion.
    let oldest = all
        .iter()
        .filter(|s| s.commit.is_some())
        .max_by_key(|s| s.commits_behind.unwrap_or(0));
    CriterionStatus {
        status: status.into(),
        reason,
        tests_draft: base.tests_draft,
        commit: oldest.and_then(|s| s.commit.clone()),
        commits_behind: oldest.and_then(|s| s.commits_behind),
        tests,
    }
}
