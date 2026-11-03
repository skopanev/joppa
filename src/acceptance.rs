//! Acceptance of a node revision on one commit, the same for every type.
//! The store checks the commit against the records before this rule runs: every present test
//! node at and below the node has a record on the commit, and it passed.
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

/// The present test nodes at and below a node, as (address, test name).
pub fn tests(w: &Workspace, key: &str) -> Vec<(String, String)> {
    crate::test_nodes::nodes_by_parent(w)
        .into_iter()
        .filter(|(parent, _)| crate::objects::within(w, parent, key))
        .flat_map(|(_, nodes)| nodes)
        .chain(
            crate::test_nodes::is_test(w, key)
                .then(|| {
                    w.objects[key].current().data["title"]
                        .as_str()
                        .map(|t| (key.to_owned(), t.to_owned()))
                })
                .flatten(),
        )
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
    let o = w.objects.get(&key).context("unknown object")?;
    ensure!(
        o.current().revision == revision,
        "stale object revision; read the object again"
    );
    ensure!(
        o.current().data["owner"].as_str() == Some(actor),
        "only the owner of the node accepts"
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
