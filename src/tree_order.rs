use crate::model::Workspace;
use anyhow::{Result, bail, ensure};
use std::collections::BTreeSet;

pub fn key(kind: &str, parent: Option<&str>) -> String {
    match parent {
        Some(parent) => format!("{kind}:{parent}"),
        None => kind.into(),
    }
}
pub fn ordered_ids(w: &Workspace, kind: &str, parent: Option<&str>) -> Result<Vec<String>> {
    let members: BTreeSet<String> = match (kind, parent) {
        ("domain", None) => w.domains.keys().cloned().collect(),
        ("capability", Some(domain)) => {
            ensure!(w.domains.contains_key(domain), "unknown parent domain");
            w.capabilities
                .iter()
                .filter(|(_, c)| c.domain == domain)
                .map(|(id, _)| id.clone())
                .collect()
        }
        ("requirement", Some(capability)) => {
            ensure!(
                w.capabilities.contains_key(capability),
                "unknown parent capability"
            );
            w.requirements
                .iter()
                .filter(|(_, r)| r.capability == capability)
                .map(|(id, _)| id.clone())
                .collect()
        }
        _ => bail!("order domains, capabilities or requirements within their parent"),
    };
    let mut ids: Vec<String> = w
        .tree_order
        .get(&key(kind, parent))
        .into_iter()
        .flatten()
        .filter(|id| members.contains(*id))
        .cloned()
        .collect();
    for id in members {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}
pub fn reorder(
    w: &mut Workspace,
    actor: &str,
    kind: &str,
    parent: Option<&str>,
    expected: &[String],
    ids: &[String],
) -> Result<()> {
    ensure!(actor == w.admin, "workspace owner reorders the tree");
    let current = ordered_ids(w, kind, parent)?;
    ensure!(expected == current, "order changed; refresh and try again");
    ensure!(
        ids.len() == current.len()
            && ids.iter().collect::<BTreeSet<_>>() == current.iter().collect::<BTreeSet<_>>(),
        "include each sibling exactly once; moving between parents is not allowed"
    );
    ensure!(ids != current, "order is unchanged");
    w.tree_order.insert(key(kind, parent), ids.to_vec());
    Ok(())
}
