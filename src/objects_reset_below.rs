//! Part of `objects`: the boundary of each criterion and test node below a changed node.
use super::*;

/// Reset each present criterion and test node at and below `root` after a change recorded at `at`.
/// Reports received at or before `at` stop counting for it. A later boundary replaces an
/// earlier one.
pub fn reset_criteria(w: &mut Workspace, root: &str, at: &str) -> Result<()> {
    reset_below(w, &[root.to_owned()], at)
}

/// `reset_criteria` for each of `roots`. One pass over the objects builds the children of
/// each node, and a walk from the roots reads only their subtrees.
pub(crate) fn reset_below(w: &mut Workspace, roots: &[String], at: &str) -> Result<()> {
    if roots.is_empty() {
        return Ok(());
    }
    let mut children: std::collections::HashMap<&str, Vec<&str>> = Default::default();
    for (key, o) in &w.objects {
        if let Some(parent) = o.current().parent.as_deref() {
            let parent = resolve_address(w, parent);
            children.entry(parent).or_default().push(key);
        }
    }
    let types = schema(w).types;
    let mut queue: Vec<&str> = roots.iter().map(|r| resolve_address(w, r)).collect();
    let mut seen = BTreeSet::new();
    let mut criteria = vec![];
    while let Some(key) = queue.pop() {
        if !seen.insert(key) {
            continue;
        }
        if let Some(o) = w.objects.get(key) {
            let counted = types
                .get(&o.kind)
                .is_some_and(|t| t.role == "ac" || t.role == crate::test_nodes::ROLE);
            if counted && o.current().present {
                criteria.push(key.to_owned());
            }
        }
        queue.extend(children.get(key).into_iter().flatten());
    }
    drop(children);
    for key in criteria {
        // An earlier or equal time keeps the stored boundary.
        let later = w.criterion_boundaries.get(&key).is_some_and(|old| {
            use crate::ac_tests::Boundary;
            matches!(
                (Boundary::read(Some(old)), Boundary::read(Some(at))),
                (Boundary::After(old), Boundary::After(new)) if old >= new
            )
        });
        if !later {
            std::sync::Arc::make_mut(&mut w.criterion_boundaries).insert(key, at.into());
        }
    }
    Ok(())
}
