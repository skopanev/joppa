//! Tests that prove a criterion, and the status they give it.
//!
//! A criterion lists its tests by name. The block test_results of the tree schema gives the
//! form of a name and the largest list. The list keeps its own history, apart from the
//! requirement revision. Each save, also an identical one, resets the criterion:
//! earlier reports stop counting and its readiness is cleared.
//! With draft_on_text_change, a change of the draft_fields of the criterion makes the
//! saved list a draft until it is saved again.
use crate::model::Workspace;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

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

/// After a text change of a criterion, its saved test list is a draft until the list
/// is saved again. The hint tells the writer this.
pub fn draft_hint(w: &crate::model::Workspace, address: &str) -> Option<String> {
    let list = listed(w, address)?;
    (!list.tests.is_empty() && list.draft).then(|| {
        format!(
            "The test list of {address} is now a draft. Send set_ac_tests {{target, tests, revision: {}, reason}} with the same list to end the draft.",
            list.revision
        )
    })
}

/// Hash of the fields that an older test list was written against, joined by new lines.
fn hash_of(fields: &[&str], data: &serde_json::Value) -> String {
    let text: Vec<&str> = fields
        .iter()
        .map(|f| data[*f].as_str().unwrap_or_default())
        .collect();
    format!("{:x}", Sha256::digest(text.join("\n").as_bytes()))
}

/// Apply `set_ac_tests`. After migrate_test_rules it saves the field tests of the current
/// document as an ordinary revision, see `set_field`. Before it, the command appended a
/// revision of its own list and reset the criterion at the save time.
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
    let saved = listed(w, &address).map(|l| l.revision);
    ensure!(
        expected.is_none_or(|e| Some(e) == saved || (e == 0 && saved.is_none())),
        "stale test list revision; read the tests again"
    );
    if w.test_rules_migrated {
        return set_field(w, actor, &address, tests, reason, position, at);
    }
    // The rules of the journals before migrate_test_rules.
    let rules = crate::test_rules::legacy();
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
        tests.len() <= rules.max_tests,
        "a criterion lists at most {} tests",
        rules.max_tests
    );
    let mut seen = BTreeSet::new();
    for test in tests {
        crate::test_rules::validate_address(&rules, test)?;
        ensure!(seen.insert(test), "test \"{test}\" is listed twice");
    }
    let text_hash = hash_of(
        &crate::test_rules::legacy_draft_fields(),
        &object.current().data,
    );
    let entry = w.ac_tests.entry(address.clone()).or_default();
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
    if !crate::commands::rule_applies(w, crate::commands::LIST_SAVE_RESET, position) {
        return Ok(());
    }
    crate::objects::reset_criteria(w, &address, at)
}

/// Save the field tests of the current document as an ordinary revision. The save checks
/// the list, and a change resets the criterion like any other change.
fn set_field(
    w: &mut Workspace,
    actor: &str,
    address: &str,
    tests: &[String],
    reason: &str,
    position: u64,
    at: &str,
) -> Result<()> {
    let o = &w.objects[address];
    let current = o.current();
    let mut data = current.data.clone();
    data["tests"] = serde_json::json!(tests);
    // A save of the same list keeps the document and resets the criterion, as every list
    // save did before.
    if data == current.data {
        return crate::objects::reset_criteria(w, address, at);
    }
    let command = crate::commands::Command::SaveObject {
        id: o.id.clone(),
        kind: o.kind.clone(),
        revision: current.revision,
        parent: current.parent.clone(),
        data,
        links: current.links.clone(),
        question_checked: None,
        reason: reason.into(),
    };
    crate::objects::save(w, actor, &command, position, at)
}

/// The test list of a node: its tests, the revision that saved them and their author.
#[derive(Clone, Debug)]
pub struct Listed {
    pub tests: Vec<String>,
    pub revision: u32,
    pub actor: String,
    /// A list before migrate_test_rules that waits for a save after a text change.
    pub draft: bool,
}

/// The test list of a node. After migrate_test_rules it is the field tests of the current
/// document. Before it, the list had its own history and a draft after a text change.
pub fn listed(w: &Workspace, address: &str) -> Option<Listed> {
    let o = w.objects.get(address)?;
    if w.test_rules_migrated {
        // A node of a type with the status rule tests always has a list: its document
        // revision is the revision of the list, also before the first test.
        let current = o.current();
        let tests_type = crate::objects::schema(w)
            .types
            .get(&o.kind)
            .is_some_and(|t| crate::objects::status_rule(t) == "tests");
        let tests: Vec<String> = match current.data.get("tests") {
            Some(list) => list
                .as_array()?
                .iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect(),
            None if tests_type => vec![],
            None => return None,
        };
        return Some(Listed {
            tests,
            revision: current.revision,
            actor: current.actor.clone(),
            draft: false,
        });
    }
    let list = w.ac_tests.get(address).and_then(AcTests::current)?;
    let legacy = crate::test_rules::legacy_draft_fields();
    Some(Listed {
        tests: list.tests.clone(),
        revision: list.revision,
        actor: list.actor.clone(),
        draft: list.text_hash != hash_of(&legacy, &o.current().data),
    })
}

/// Refuse a field tests on a type without the status rule tests, or a bad test list.
pub fn validate_field(w: &Workspace, kind: &str, data: &serde_json::Value) -> Result<()> {
    let Some(value) = data.get("tests") else {
        return Ok(());
    };
    let t = crate::objects::schema(w);
    ensure!(
        t.types
            .get(kind)
            .is_some_and(|t| crate::objects::status_rule(t) == "tests"),
        "tests are listed on criteria, not on {kind}s"
    );
    let rules = crate::test_rules::rules(w)?;
    let tests = value.as_array().context("tests is a list of test names")?;
    ensure!(
        tests.len() <= rules.max_tests,
        "a criterion lists at most {} tests",
        rules.max_tests
    );
    let mut seen = BTreeSet::new();
    for test in tests {
        let test = test.as_str().context("tests is a list of test names")?;
        crate::test_rules::validate_address(&rules, test)?;
        ensure!(seen.insert(test), "test \"{test}\" is listed twice");
    }
    Ok(())
}

#[path = "ac_tests_status.rs"]
mod status;
pub use status::{Boundary, CriterionStatus, Report, TestResult, TestStatus, criterion_status};
