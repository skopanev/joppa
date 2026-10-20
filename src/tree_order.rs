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
    let members: BTreeSet<String> = if w.documents_migrated {
        let schema = crate::objects::schema(w);
        ensure!(schema.types.contains_key(kind), "unknown object type");
        let parent = if let Some(parent) = parent {
            let address = crate::objects::resolve_address(w, parent);
            if w.objects.get(address).is_some_and(|o| o.current().present) {
                Some(address.to_owned())
            } else {
                let mut found = w
                    .objects
                    .iter()
                    .filter(|(_, o)| o.current().present && o.id == parent);
                let first = found.next().map(|(key, _)| key.clone());
                ensure!(
                    first.is_some() && found.next().is_none(),
                    "use a complete parent address"
                );
                first
            }
        } else {
            None
        };
        w.objects
            .values()
            .filter(|o| {
                o.kind == kind
                    && o.current().present
                    && o.current().data["archived"] != true
                    && o.current().parent == parent
            })
            .map(|o| o.id.clone())
            .collect()
    } else {
        match (kind, parent) {
            ("domain", None) => w
                .domains
                .iter()
                .filter(|(_, d)| !d.archived)
                .map(|(id, _)| id.clone())
                .collect(),
            ("capability", Some(domain)) => {
                ensure!(w.domains.contains_key(domain), "unknown parent domain");
                w.capabilities
                    .iter()
                    .filter(|(_, c)| c.domain.as_deref() == Some(domain))
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
                    .filter(|(_, r)| r.capability.as_deref() == Some(capability))
                    .map(|(id, _)| id.clone())
                    .collect()
            }
            _ => bail!("order domains, capabilities or requirements within their parent"),
        }
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
    ensure!(
        crate::objects::may_edit_tree(w, actor),
        "workspace member required"
    );
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
