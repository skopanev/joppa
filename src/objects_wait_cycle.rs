//! Part of `objects`: the wait cycle check of depends_on edges. A Task can depend on a Task
//! under any requirement; only a dependency cycle is refused.
use super::*;

/// A live save refuses a new depends_on edge from S to T when T, or a node below T, waits on
/// S, on an ancestor of S or on a node below S. A node waits on the depends_on targets of
/// itself and of its ancestors, and a target needs itself and each live node below it.
/// The rule is the same for every type of node, as in `dependencies::Graph`. Replay accepts
/// the edges that are already stored.
pub(crate) fn validate_wait_cycle(
    w: &Workspace,
    key: &str,
    parent: Option<&str>,
    relations: &crate::relations::Relations,
) -> Result<()> {
    use crate::relations::{DEPENDS_ON, edges_of};
    let source = resolve_address(w, key).to_owned();
    let none = crate::relations::Relations::new();
    let stored = w.objects.get(&source).map(|o| &o.current().relations);
    let stored = stored.unwrap_or(&none);
    let added: Vec<String> = edges_of(relations, DEPENDS_ON)
        .filter(|e| !edges_of(stored, DEPENDS_ON).any(|s| s.to == e.to))
        .map(|e| resolve_address(w, &e.to).to_owned())
        .collect();
    if added.is_empty() {
        return Ok(());
    }
    let live = |k: &str| w.objects.get(k).is_some_and(|o| o.current().present);
    let parent_of = |k: &str| -> Option<String> {
        if k == source {
            parent.map(|p| resolve_address(w, p).to_owned())
        } else {
            w.objects
                .get(k)
                .and_then(|o| o.current().parent.as_deref())
                .map(|p| resolve_address(w, p).to_owned())
        }
    };
    let mut children = BTreeMap::<String, Vec<String>>::new();
    for k in w.objects.keys().filter(|k| live(k)).chain([&source]) {
        if let Some(p) = parent_of(k) {
            children.entry(p).or_default().push(k.clone());
        }
    }
    let ancestors = |k: &str| {
        let mut found = vec![];
        let mut next = parent_of(k);
        while let Some(p) = next.filter(|p| *p != k && !found.contains(p)) {
            next = parent_of(&p);
            found.push(p);
        }
        found
    };
    let below = |k: &str| {
        let mut found = BTreeSet::new();
        let mut stack = vec![k.to_owned()];
        while let Some(n) = stack.pop() {
            for c in children.get(&n).into_iter().flatten() {
                if found.insert(c.clone()) {
                    stack.push(c.clone());
                }
            }
        }
        found
    };
    let targets = |k: &str| -> Vec<String> {
        let own = if k == source {
            relations
        } else {
            w.objects.get(k).map_or(&none, |o| &o.current().relations)
        };
        edges_of(own, DEPENDS_ON)
            .map(|e| resolve_address(w, &e.to).to_owned())
            .filter(|t| live(t))
            .collect()
    };
    let mut cycle: BTreeSet<String> = below(&source);
    cycle.extend(ancestors(&source));
    cycle.insert(source.clone());
    let mut visited = BTreeSet::new();
    let mut needed = added;
    while let Some(target) = needed.pop() {
        ensure!(!cycle.contains(&target), "dependency cycle");
        let mut nodes = below(&target);
        nodes.insert(target);
        for node in nodes {
            if visited.insert(node.clone()) {
                for waiting in std::iter::once(node.clone()).chain(ancestors(&node)) {
                    needed.extend(targets(&waiting));
                }
            }
        }
    }
    Ok(())
}
