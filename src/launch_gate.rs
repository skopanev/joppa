//! The administrative launch gate. Each launch condition belongs to a requirement, and the
//! owner accepted that requirement with accept_commit on the commit of the release.
use crate::model::Workspace;
use crate::objects::{launch_objects, resolve_address, schema};
use anyhow::{Result, ensure};
use std::collections::BTreeSet;

/// The address of the nearest requirement object at or above `key`.
pub fn requirement_object<'a>(w: &'a Workspace, key: &str) -> Option<&'a str> {
    let types = schema(w).types;
    let mut current = w.objects.get_key_value(resolve_address(w, key));
    let mut seen = BTreeSet::new();
    while let Some((address, o)) = current {
        if !seen.insert(address.as_str()) {
            return None;
        }
        if !w.document_overrides.contains(address)
            && types.get(&o.kind).is_some_and(|t| t.role == "requirement")
        {
            return Some(address);
        }
        current = o
            .current()
            .parent
            .as_deref()
            .and_then(|parent| w.objects.get_key_value(resolve_address(w, parent)));
    }
    None
}

/// The commit of the current acceptance of the requirement of `key`, if the owner accepted it.
pub fn accepted_commit(w: &Workspace, key: &str) -> Option<String> {
    let requirement = requirement_object(w, key)?;
    crate::acceptance::current(w, requirement)["commit"]
        .as_str()
        .map(str::to_owned)
}

/// Pass only when the workspace has launch conditions and each one is accepted on `commit`.
/// Returns the number of launch conditions.
pub fn check(w: &Workspace, commit: &str) -> Result<usize> {
    ensure!(!commit.trim().is_empty(), "gate needs a commit");
    let conditions = launch_objects(w);
    ensure!(
        !conditions.is_empty(),
        "the workspace has no launch conditions"
    );
    for key in &conditions {
        match accepted_commit(w, key) {
            Some(accepted) if accepted == commit => {}
            Some(accepted) => {
                anyhow::bail!(
                    "launch condition {key} is accepted on commit {accepted}, not {commit}"
                )
            }
            None => anyhow::bail!("launch condition {key} has no acceptance on a commit"),
        }
    }
    Ok(conditions.len())
}
