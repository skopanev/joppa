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
use crate::objects::{Object, ObjectRevision, contract_revision, schema, status_rule};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

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
    let kinds = kinds(w);
    let mut names = std::collections::BTreeMap::<String, Vec<String>>::new();
    for o in w.objects.values().filter(|o| kinds.contains(&o.kind)) {
        let current = o.current();
        if let (true, Some(parent), Some(name)) = (
            current.present,
            &current.parent,
            current.data["title"].as_str(),
        ) {
            let parent = crate::objects::resolve_address(w, parent).to_owned();
            names.entry(parent).or_default().push(name.to_owned());
        }
    }
    names
}

/// The ID of the node of one test under one parent: 12 hex characters of a SHA-256.
pub fn node_id(parent: &str, name: &str) -> String {
    crate::digest_hex::sha256_hex(format!("{parent}\n{name}"))[..12].to_owned()
}

/// After the event, a document has no test list, and the title of a test node is a test name
/// that the name rule of the schema accepts.
pub fn check_save(w: &Workspace, kind: &str, data: &Value) -> Result<()> {
    if !w.test_nodes_migrated {
        return Ok(());
    }
    ensure!(data.get("tests").is_none(), REFUSAL);
    if kinds(w).iter().any(|k| k == kind) {
        let name = data["title"]
            .as_str()
            .context("a test node needs a title")?;
        crate::test_rules::validate_address(&crate::test_rules::rules(w)?, name)?;
    }
    Ok(())
}

/// Apply migrate_test_nodes once. The workspace owner is the actor of the event.
pub fn migrate(w: &mut Workspace, actor: &str, position: u64, at: &str) -> Result<()> {
    if w.test_nodes_migrated {
        return Ok(());
    }
    ensure!(actor == w.admin, "workspace owner required");
    let mut next = schema(w);
    let listing: Vec<String> = next
        .types
        .iter()
        .filter(|(_, t)| t.role == "ac" || status_rule(t) == "tests")
        .map(|(kind, _)| kind.clone())
        .collect();
    let kind = match next.types.iter().find(|(_, t)| t.role == ROLE) {
        Some((kind, _)) => kind.clone(),
        None => {
            let kind = ["test", "test_case", "test_node"]
                .into_iter()
                .find(|k| !next.types.contains_key(*k))
                .context("no free name for the test type")?;
            let t = json!({"title":"Tests","role":ROLE,"status":RULE,"parents":listing,
                "badge":"TEST","title_preview_length":400});
            next.types.insert(kind.into(), serde_json::from_value(t)?);
            kind.to_owned()
        }
    };
    for k in &listing {
        let t = next.types.get_mut(k).expect("listed type");
        t.status = Some("children".into());
    }
    next.revision += 1;
    next.actor = actor.into();
    next.at = at.into();
    next.position = position;
    next.reason = REASON.into();
    let schema_revision = next.revision;
    w.tree_schema.push(next);
    let lists: Vec<(String, Vec<String>)> = w
        .objects
        .iter()
        .filter(|(_, o)| o.current().present)
        .filter_map(|(key, o)| {
            let tests = o.current().data.get("tests")?.as_array()?;
            Some((
                key.clone(),
                tests
                    .iter()
                    .filter_map(|t| t.as_str().map(String::from))
                    .collect(),
            ))
        })
        .collect();
    for (key, tests) in lists {
        let o = w.objects.get_mut(&key).expect("listed object");
        let contract = contract_revision(o);
        let mut revision = o.current().clone();
        revision.data.as_object_mut().map(|d| d.remove("tests"));
        revision.revision += 1;
        revision.contract = Some([revision.revision, contract]);
        revision.schema_revision = schema_revision;
        revision.position = position;
        revision.actor = actor.into();
        revision.reason = REASON.into();
        revision.at = at.into();
        let owner = revision.data["owner"].clone();
        o.revisions.push(revision);
        let boundary = w.criterion_boundaries.get(&key).cloned();
        for name in tests {
            let id = node_id(&key, &name);
            let address = crate::objects::address(&kind, &id);
            let node = ObjectRevision {
                revision: 1,
                schema_revision,
                parent: Some(key.clone()),
                data: json!({"title": name, "owner": owner}),
                links: vec![],
                actor: actor.into(),
                at: at.into(),
                position,
                reason: REASON.into(),
                present: true,
                contract: None,
            };
            w.objects.insert(
                address.clone(),
                Object {
                    kind: kind.clone(),
                    id,
                    revisions: vec![node],
                    parent_override: false,
                },
            );
            if let Some(at) = &boundary {
                w.criterion_boundaries.insert(address, at.clone());
            }
        }
    }
    w.test_nodes_migrated = true;
    Ok(())
}
