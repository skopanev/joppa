//! Acceptance of a requirement revision on one commit.
//! The store checks the commit against the records before this rule runs:
//! every listed test of every criterion has a record on the commit, and it passed.
//! Here the rule checks who accepts and binds the acceptance to the revision and the commit.
use crate::model::Workspace;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Acceptance {
    pub revision: u32,
    pub commit: String,
    pub actor: String,
    pub at: String,
    pub position: u64,
}

/// The present criteria directly under a requirement.
pub fn criteria(w: &Workspace, key: &str) -> Vec<String> {
    w.objects
        .iter()
        .filter(|(_, o)| {
            let r = o.current();
            r.present
                && r.parent.as_deref() == Some(key)
                && crate::objects::schema(w)
                    .types
                    .get(&o.kind)
                    .is_some_and(|t| t.role == "ac")
        })
        .map(|(address, _)| address.clone())
        .collect()
}

pub fn accept(
    w: &mut Workspace,
    actor: &str,
    target: &str,
    revision: u32,
    commit: Option<&str>,
    position: u64,
    at: &str,
) -> Result<()> {
    crate::commands::person(w, actor)?;
    let commit = commit
        .filter(|c| !c.trim().is_empty())
        .context("acceptance needs a commit")?;
    let key = crate::objects::resolve_address(w, target).to_owned();
    let o = w.objects.get(&key).context("unknown requirement")?;
    ensure!(
        crate::objects::schema(w)
            .types
            .get(&o.kind)
            .is_some_and(|t| t.role == "requirement"),
        "acceptance is for a requirement"
    );
    ensure!(
        o.current().revision == revision,
        "stale object revision; read the object again"
    );
    ensure!(
        o.current().data["owner"].as_str() == Some(actor),
        "only the requirement owner or its decider accepts"
    );
    w.acceptances.entry(key).or_default().push(Acceptance {
        revision,
        commit: commit.into(),
        actor: actor.into(),
        at: at.into(),
        position,
    });
    Ok(())
}

/// The acceptance of the current revision; a new revision is not accepted by an old acceptance.
pub fn current(w: &Workspace, key: &str) -> Value {
    let Some(o) = w.objects.get(key) else {
        return Value::Null;
    };
    w.acceptances
        .get(key)
        .and_then(|all| {
            all.iter()
                .rev()
                .find(|a| a.revision == o.current().revision)
        })
        .map_or(Value::Null, |a| json!(a))
}
