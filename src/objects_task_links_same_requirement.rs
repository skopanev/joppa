//! Part of `objects`: `task_links_same_requirement` and the items after it, moved from objects.rs unchanged.
use super::*;

/// A live save may add a blocking Task link only to a Task under the same requirement.
/// Replay skips this check, so stored links keep loading.
pub(crate) fn task_links_same_requirement(w: &Workspace, cmd: &Command) -> Result<()> {
    let Command::SaveObject(crate::commands::SaveObjectOp {
        id,
        kind,
        parent,
        links,
        ..
    }) = cmd
    else {
        return Ok(());
    };
    if !task_kind(w, kind) {
        return Ok(());
    }
    // A stored blocking link stays allowed when the save only moves its pin.
    let stored: BTreeSet<&str> = w
        .objects
        .get(resolve_address(w, &address(kind, id)))
        .into_iter()
        .flat_map(|o| o.current().links.iter().filter(|l| l.blocking))
        .map(|l| resolve_address(w, &l.target))
        .collect();
    // The candidate state: the Task sits under its proposed parent.
    let req = parent
        .as_deref()
        .and_then(|parent| crate::tasks::requirement(w, parent));
    for link in links
        .iter()
        .filter(|l| l.blocking && !stored.contains(resolve_address(w, &l.target)))
    {
        let Some(target) = w.objects.get(resolve_address(w, &link.target)) else {
            continue;
        };
        if task_kind(w, &target.kind) {
            ensure!(
                crate::tasks::requirement(w, &link.target) == req,
                "a Task can depend only on a Task under the same requirement"
            );
        }
    }
    Ok(())
}

/// A live save refuses a new blocking link from S to T when T, or a node below T, waits on S,
/// on an ancestor of S or on a node below S. A node waits on the blocking link targets of
/// itself and of its ancestors, and a target needs itself and each live node below it.
/// Task links keep their revision pins and do not take part, as in `dependencies::Graph`.
/// Replay accepts the links that are already stored.
pub(crate) fn validate_wait_cycle(
    w: &Workspace,
    key: &str,
    parent: Option<&str>,
    links: &[ObjectLink],
) -> Result<()> {
    let task = |k: &str| {
        k.split_once(':')
            .is_some_and(|(kind, _)| task_kind(w, kind))
    };
    if task(key) {
        return Ok(());
    }
    let source = resolve_address(w, key).to_owned();
    let stored = w
        .objects
        .get(&source)
        .map(|o| o.current().links.as_slice())
        .unwrap_or_default();
    let added: Vec<String> = links
        .iter()
        .filter(|l| l.blocking && !stored.iter().any(|s| s.blocking && s.target == l.target))
        .map(|l| resolve_address(w, &l.target).to_owned())
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
        let own = if task(k) {
            &[]
        } else if k == source {
            links
        } else {
            w.objects
                .get(k)
                .map(|o| o.current().links.as_slice())
                .unwrap_or_default()
        };
        own.iter()
            .filter(|l| l.blocking)
            .map(|l| resolve_address(w, &l.target).to_owned())
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
