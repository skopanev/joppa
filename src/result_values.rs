//! The status of a criterion for each value of a board field of the result shape.
//!
//! A board field splits the results of a criterion by its value. Each value gets its own
//! status from the results with that value: the latest result of a test for that value
//! decides the test. The criterion status combines the values: red when one value is red,
//! green when every value is green, and grey otherwise. A value without results is grey.
//! A result without the field counts for every value, such as a shared backend test.
//! A board field can name the reference field of a criterion that lists the values that
//! apply to it, or holds the explicit value all. A value that does not apply has the status
//! n/a and does not count. A workspace without a board field keeps the status of all its
//! results.
use crate::ac_tests::{Boundary, CriterionStatus, Report, TestResult, criterion_status};
use crate::result_shape::ResultShape;
use anyhow::Result;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

/// A board field with its values: the enum values in the schema order, or the present
/// objects of the target type of a ref field.
#[derive(Clone, Debug)]
pub struct BoardField {
    pub name: String,
    pub values: Vec<String>,
    pub applies: Option<String>,
}

/// The board fields of the block of the workspace.
pub fn board_fields(shape: &ResultShape, w: &crate::model::Workspace) -> Vec<BoardField> {
    shape
        .fields
        .iter()
        .filter(|f| f.board)
        .map(|f| BoardField {
            name: f.name.clone(),
            values: match &f.target {
                Some(target) => w
                    .objects
                    .values()
                    .filter(|o| &o.kind == target && o.current().present)
                    .map(|o| o.id.clone())
                    .collect(),
                None => f.values.clone(),
            },
            applies: f.applies.clone(),
        })
        .collect()
}

/// The values of a board field that apply to a criterion document.
fn applying(field: &BoardField, data: &Value) -> BTreeSet<String> {
    let all = || field.values.iter().cloned().collect();
    let Some(name) = &field.applies else {
        return all();
    };
    match data.get(name) {
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(Value::as_str)
            .map(|v| v.rsplit(':').next().unwrap_or(v).to_owned())
            .collect(),
        Some(Value::String(v)) if v != "all" => {
            [v.rsplit(':').next().unwrap_or(v).to_owned()].into()
        }
        _ => all(),
    }
}

/// Test statuses from the worst to the best. The criterion shows the worst of the values.
const WORST_FIRST: [&str; 5] = ["fail", "not_run", "skipped", "todo", "pass"];
/// The reasons of a grey status that do not depend on the results.
const OWN_REASONS: [&str; 3] = ["no tests", "tests_draft", "the reset time cannot be read"];

/// The criterion status with `values`: for each board field, the status of each value.
#[allow(clippy::too_many_arguments)]
pub fn status_by_value(
    tests: &[String],
    draft: bool,
    boundary: Boundary,
    reports: &[Report],
    own: &[(i64, TestResult)],
    rules: &ResultShape,
    board: &[BoardField],
    data: &Value,
) -> Result<Value> {
    let base = criterion_status(tests, draft, boundary, reports, own, rules);
    if board.is_empty() {
        return Ok(serde_json::to_value(base)?);
    }
    let mut values = Map::new();
    let mut all = vec![];
    for field in board {
        let applies = applying(field, data);
        let mut by_value = Map::new();
        for value in &field.values {
            if !applies.contains(value) {
                by_value.insert(value.clone(), json!({"status": "n/a"}));
                continue;
            }
            let mine: Vec<(i64, TestResult)> = own
                .iter()
                .filter(|(_, r)| {
                    r.fields
                        .as_ref()
                        .and_then(|f| f.get(&field.name))
                        .and_then(Value::as_str)
                        .is_none_or(|v| v == value)
                })
                .cloned()
                .collect();
            let status = criterion_status(tests, draft, boundary, reports, &mine, rules);
            by_value.insert(value.clone(), serde_json::to_value(&status)?);
            all.push(status);
        }
        values.insert(field.name.clone(), Value::Object(by_value));
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
