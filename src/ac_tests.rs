//! Tests that prove a criterion, and the status they give it.
//!
//! A criterion lists its tests by address: the file, the describe blocks and the
//! test name joined by " > ". The list keeps its own history, apart from the
//! requirement revision, so saving it never resets readiness or acceptance.
//! A change of the criterion text makes the saved list a draft until it is saved again.
use crate::model::Workspace;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const SEPARATOR: &str = " > ";
const MAX_TESTS: usize = 200;
const MAX_ADDRESS: usize = 400;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcTestsRevision {
    pub revision: u32,
    pub tests: Vec<String>,
    /// Hash of the criterion title and description when the list was saved.
    pub text_hash: String,
    pub actor: String,
    pub at: String,
    pub position: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AcTests {
    pub revisions: Vec<AcTestsRevision>,
}

impl AcTests {
    pub fn current(&self) -> Option<&AcTestsRevision> {
        self.revisions.last()
    }
}

pub fn validate_address(address: &str) -> Result<()> {
    let parts: Vec<&str> = address.split(SEPARATOR).collect();
    // The first part is the test file: a path with a file extension, such as tests/login.test.ts.
    let file = parts[0].rsplit('/').next().unwrap_or_default();
    ensure!(
        parts.len() >= 2
            && parts.iter().all(|part| !part.trim().is_empty())
            && file.contains('.')
            && !file.starts_with('.')
            && address.chars().count() <= MAX_ADDRESS
            && !address.chars().any(char::is_control),
        "a test address needs file > name at least"
    );
    Ok(())
}

/// Hash of the text that the test list was written against.
pub fn text_hash(data: &serde_json::Value) -> String {
    let text = format!(
        "{}\n{}",
        data["title"].as_str().unwrap_or_default(),
        data["description"].as_str().unwrap_or_default()
    );
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Apply `set_ac_tests`: validate the target and the addresses, then append a list revision.
/// A supplied `expected` revision must equal the current list revision, or 0 without a list.
#[allow(clippy::too_many_arguments)]
pub fn set(
    w: &mut Workspace,
    actor: &str,
    target: &str,
    tests: &[String],
    expected: Option<u32>,
    reason: &str,
    position: u64,
    at: &str,
) -> Result<()> {
    crate::commands::nonempty(reason)?;
    let address = crate::objects::resolve_address(w, target).to_owned();
    let object = w.objects.get(&address).context("unknown criterion")?;
    ensure!(
        object.current().present,
        "criterion is absent from the current tree"
    );
    let role = crate::objects::schema(w)
        .types
        .get(&object.kind)
        .map(|t| t.role.clone());
    ensure!(
        role.as_deref() == Some("ac"),
        "tests are listed on criteria, not on {}s",
        object.kind
    );
    ensure!(
        tests.len() <= MAX_TESTS,
        "a criterion lists at most {MAX_TESTS} tests"
    );
    let mut seen = BTreeSet::new();
    for test in tests {
        validate_address(test)?;
        ensure!(seen.insert(test), "test \"{test}\" is listed twice");
    }
    let text_hash = text_hash(&object.current().data);
    let current = w
        .ac_tests
        .get(&address)
        .and_then(AcTests::current)
        .map_or(0, |r| r.revision);
    ensure!(
        expected.is_none_or(|expected| expected == current),
        "stale test list revision; read the tests again"
    );
    let entry = w.ac_tests.entry(address).or_default();
    let revision = entry.revisions.len() as u32 + 1;
    entry.revisions.push(AcTestsRevision {
        revision,
        tests: tests.to_vec(),
        text_hash,
        actor: actor.into(),
        at: at.into(),
        position,
        reason: reason.into(),
    });
    Ok(())
}

#[path = "ac_tests_junit.rs"]
mod junit;
pub use junit::{TestResult, parse_junit};
#[path = "ac_tests_status.rs"]
mod status;
pub use status::{CriterionStatus, Report, TestStatus, criterion_status};
