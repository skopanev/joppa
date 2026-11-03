//! A test is a node of the tree. Its type has the role `test` and the status rule `own`: the
//! latest result of the test whose name is the title of the node decides its tone. One node
//! names one test; a test that proves two criteria has one node under each.
//!
//! The event migrate_test_nodes runs once in each workspace:
//! - The schema gets the type `test` (role test, status own) under each type with role ac or
//!   status tests, unless a type with role test exists. Those types take the status children.
//! - Each name in the list `data.tests` of a present object becomes a test node under it, with
//!   the owner of the object. The list leaves the document in a new revision that keeps the
//!   contract revision, so pins and Task reviews stay.
//! - Each new node keeps the reset boundary of its parent, so its result still counts.
//!
//! After the event a save with `data.tests` and the operation set_ac_tests are refused.
use crate::model::Workspace;
use crate::objects::schema;
use anyhow::{Context, Result, ensure};
use serde_json::Value;

#[path = "test_nodes_migration.rs"]
mod migration;
pub use migration::migrate;

pub const ROLE: &str = "test";
pub const RULE: &str = "own";
pub const REASON: &str = "A test is a node of the tree";
/// The refusal of a test list after the event.
pub const REFUSAL: &str = "use save_object with kind test";

/// The types with the role test.
pub fn kinds(w: &Workspace) -> Vec<String> {
    let types = schema(w).types;
    types
        .into_iter()
        .filter(|(_, t)| t.role == ROLE)
        .map(|(kind, _)| kind)
        .collect()
}

/// True when the object is a test node.
pub fn is_test(w: &Workspace, address: &str) -> bool {
    w.objects
        .get(address)
        .is_some_and(|o| kinds(w).contains(&o.kind))
}

/// The names of the present test nodes under each parent, in address order.
pub fn names_by_parent(w: &Workspace) -> std::collections::BTreeMap<String, Vec<String>> {
    let nodes = nodes_by_parent(w).into_iter();
    nodes
        .map(|(parent, nodes)| (parent, nodes.into_iter().map(|(_, name)| name).collect()))
        .collect()
}

/// The present test nodes under each parent, as (address, test name), in address order.
pub fn nodes_by_parent(w: &Workspace) -> std::collections::BTreeMap<String, Vec<(String, String)>> {
    let kinds = kinds(w);
    let mut nodes = std::collections::BTreeMap::<String, Vec<(String, String)>>::new();
    for (address, o) in w.objects.iter().filter(|(_, o)| kinds.contains(&o.kind)) {
        let current = o.current();
        if let (true, Some(parent), Some(name)) = (
            current.present,
            &current.parent,
            current.data["title"].as_str(),
        ) {
            let parent = crate::objects::resolve_address(w, parent).to_owned();
            nodes
                .entry(parent)
                .or_default()
                .push((address.clone(), name.to_owned()));
        }
    }
    nodes
}

/// The ID of the node of one test under one parent: 12 hex characters of a SHA-256.
pub fn node_id(parent: &str, name: &str) -> String {
    crate::digest_hex::sha256_hex(format!("{parent}\n{name}"))[..12].to_owned()
}

/// After the event, a document has no test list. The title of a test node is a test name that
/// the name rule of the schema accepts, it is unique among the test nodes of its parent, and a
/// parent has at most `max_tests` test nodes.
pub fn check_save(
    w: &Workspace,
    kind: &str,
    key: &str,
    parent: Option<&str>,
    data: &Value,
) -> Result<()> {
    if !w.test_nodes_migrated {
        return Ok(());
    }
    ensure!(data.get("tests").is_none(), REFUSAL);
    let kinds = kinds(w);
    if !kinds.iter().any(|k| k == kind) {
        return Ok(());
    }
    let rules = crate::test_rules::rules(w)?;
    let name = data["title"]
        .as_str()
        .context("a test node needs a title")?;
    crate::test_rules::validate_address(&rules, name)?;
    let siblings: Vec<&str> = w
        .objects
        .iter()
        .filter(|(address, o)| {
            let current = o.current();
            address.as_str() != key
                && kinds.contains(&o.kind)
                && current.present
                && current
                    .parent
                    .as_deref()
                    .map(|p| crate::objects::resolve_address(w, p))
                    == parent
        })
        .filter_map(|(_, o)| o.current().data["title"].as_str())
        .collect();
    ensure!(!siblings.contains(&name), "test \"{name}\" is listed twice");
    ensure!(
        siblings.len() < rules.max_tests,
        "a criterion lists at most {} tests",
        rules.max_tests
    );
    Ok(())
}

/// The status of a node from the statuses of its test nodes: red when one is red, green when
/// every one is green, else grey. It shows the oldest commit among them.
pub fn combine(parts: Vec<crate::ac_tests::CriterionStatus>) -> crate::ac_tests::CriterionStatus {
    let (status, reason) = if parts.is_empty() {
        ("grey", Some("no tests".to_owned()))
    } else if parts.iter().any(|p| p.status == "red") {
        ("red", None)
    } else if parts.iter().all(|p| p.status == "green") {
        ("green", None)
    } else {
        let reason = parts
            .iter()
            .find(|p| p.status != "green")
            .and_then(|p| p.reason.clone());
        ("grey", reason)
    };
    let oldest = parts
        .iter()
        .filter(|p| p.oldest_order.is_some())
        .min_by_key(|p| p.oldest_order);
    crate::ac_tests::CriterionStatus {
        status: status.into(),
        reason,
        tests_draft: false,
        commit: oldest.and_then(|p| p.commit.clone()),
        oldest_order: oldest.and_then(|p| p.oldest_order),
        tests: parts.into_iter().flat_map(|p| p.tests).collect(),
    }
}

/// True when the actor saved or archived a test node below `parent` after `position`.
pub fn saved_since(w: &Workspace, parent: &str, actor: &str, position: u64) -> bool {
    let kinds = kinds(w);
    w.objects.values().any(|o| {
        let p = o
            .current()
            .parent
            .as_deref()
            .map(|p| crate::objects::resolve_address(w, p));
        kinds.contains(&o.kind)
            && p == Some(parent)
            && o.revisions
                .iter()
                .any(|r| r.actor == actor && r.position > position)
    })
}
