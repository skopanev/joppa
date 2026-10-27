//! The test rules of a workspace: the block `test_results` of the tree schema.
//!
//! The block holds every rule of tests and results. It gives the status names of a
//! result and the outcome that each name counts as, the fields of a result, the form of a
//! test name, the limits, and the rules that make a criterion green, red or grey. The
//! live code reads only this block. The event migrate_test_rules writes the block into
//! each workspace, see test_rules. The fields test, status and commit are the frame of a
//! record, and pass, fail, skip and todo are the outcomes that the rules use.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The outcomes that a status name can count as.
pub const OUTCOMES: [&str; 4] = ["pass", "fail", "skip", "todo"];
/// Field names that the record or the server already uses.
pub(crate) const RESERVED: [&str; 5] = ["test", "status", "commit", "actor", "at"];
pub(crate) const FIELD_TYPES: [&str; 4] = ["text", "number", "enum", "ref"];
const SAME_COMMIT: [&str; 2] = ["fail_wins", "latest_wins"];
pub(crate) const MAX_FIELDS: usize = 20;
const MAX_STATUSES: usize = 20;
pub(crate) const MAX_VALUES: usize = 50;
/// The largest limit that the schema can set for a text field or a test name.
pub(crate) const MAX_TEXT: usize = 64 * 1024;

/// A missing key reads as empty only in a block that an older journal saved. The event
/// migrate_test_rules completes that block, and set_tree_schema refuses an incomplete one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ResultShape {
    /// Each accepted status name with the outcome that it counts as.
    pub statuses: BTreeMap<String, String>,
    /// The fields of a result, in the order that readers show them.
    pub fields: Vec<ResultField>,
    /// The form of a test name.
    pub name: NameRule,
    /// The largest number of tests in the list of one criterion.
    pub max_tests: usize,
    /// The largest number of results in one record.
    pub max_results: usize,
    /// fail_wins: a failure in any record of a commit decides the test on that commit.
    /// latest_wins: only the newest record decides the test.
    pub same_commit: String,
    /// A change at or above a criterion makes the earlier records stop counting.
    pub reset_on_change: bool,
    /// A criterion is green when the outcome of every listed test is in this list.
    pub green_if_all: Vec<String>,
    /// A criterion is red when the outcome of one listed test is in this list.
    pub red_if_any: Vec<String>,
    /// The text field that readers show as the message of a failure.
    pub failure_field: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NameRule {
    /// The text between the parts of a name, such as the file and the test.
    pub separator: String,
    /// The smallest number of parts.
    pub min_parts: usize,
    /// The first part is a file name with an extension, such as tests/login.rs.
    pub file_extension: bool,
    /// The largest number of characters.
    pub max_length: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultField {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// One of text, number, enum or ref.
    #[serde(rename = "type")]
    pub kind: String,
    /// The type of the schema whose objects a ref field names, such as asset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    /// The permitted values of an enum field. Other types have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    /// A longer text value keeps its first max_length characters. Only a text field has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    /// The board filters by this enum or ref field: each value gets its own status. A result
    /// without the field counts for every value.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub board: bool,
    /// The reference field of a criterion that lists the values that apply to it, or holds
    /// the explicit value all. Only a board field has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applies: Option<String>,
}

/// The keys that a block in set_tree_schema must have.
pub const KEYS: [&str; 10] = [
    "statuses",
    "fields",
    "name",
    "max_tests",
    "max_results",
    "same_commit",
    "reset_on_change",
    "green_if_all",
    "red_if_any",
    "failure_field",
];

pub(crate) fn field_name_ok(name: &str) -> bool {
    (1..=40).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

pub(crate) fn text_ok(value: &str) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= 1024
        && !value.chars().any(char::is_control)
}

fn outcomes_ok(list: &[String]) -> bool {
    let mut seen = BTreeSet::new();
    !list.is_empty()
        && list
            .iter()
            .all(|o| OUTCOMES.contains(&o.as_str()) && seen.insert(o))
}

/// Refuse a block with a bad status name, field, name rule, limit or criterion rule.
pub fn validate(shape: &ResultShape) -> Result<()> {
    let statuses = &shape.statuses;
    ensure!(
        (1..=MAX_STATUSES).contains(&statuses.len()),
        "test_results.statuses has 1 to {MAX_STATUSES} names"
    );
    for (name, outcome) in statuses {
        ensure!(
            (1..=32).contains(&name.chars().count())
                && name.chars().all(|c| !c.is_whitespace() && !c.is_control()),
            "a test status name has 1 to 32 characters without spaces"
        );
        ensure!(
            OUTCOMES.contains(&outcome.as_str()),
            "status {name} must count as pass, fail, skip or todo"
        );
    }
    for needed in ["pass", "fail"] {
        ensure!(
            statuses.values().any(|o| o == needed),
            "test_results.statuses needs a name that counts as {needed}"
        );
    }
    let name = &shape.name;
    ensure!(
        text_ok(&name.separator)
            && (1..=10).contains(&name.min_parts)
            && (1..=MAX_TEXT).contains(&name.max_length),
        "test_results.name needs a separator, min_parts from 1 to 10 and max_length from 1 to {MAX_TEXT}"
    );
    ensure!(
        (1..=1000).contains(&shape.max_tests) && (1..=1000).contains(&shape.max_results),
        "test_results.max_tests and max_results are from 1 to 1000"
    );
    ensure!(
        SAME_COMMIT.contains(&shape.same_commit.as_str()),
        "test_results.same_commit is fail_wins or latest_wins"
    );
    ensure!(
        outcomes_ok(&shape.green_if_all) && outcomes_ok(&shape.red_if_any),
        "test_results.green_if_all and red_if_any list different outcomes"
    );
    ensure!(
        shape
            .green_if_all
            .iter()
            .all(|o| !shape.red_if_any.contains(o)),
        "an outcome cannot be in green_if_all and red_if_any"
    );
    crate::result_check::validate_fields(&shape.fields)?;
    ensure!(
        shape.failure_field.as_ref().is_none_or(|f| shape
            .fields
            .iter()
            .any(|x| &x.name == f && x.kind == "text")),
        "test_results.failure_field names a text field of the block"
    );
    Ok(())
}
