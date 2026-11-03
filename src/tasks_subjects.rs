//! The subjects of a Task: the project references of the Task and of its ancestors.
use super::*;

pub fn subjects(w: &Workspace, key: &str) -> Result<Vec<String>> {
    if !w.schema_migrated {
        return Ok(task(w, key).map(|t| t.projects.clone()).unwrap_or_default());
    }
    let mut current = Some(key);
    let mut seen = std::collections::BTreeSet::new();
    let mut subjects = vec![];
    while let Some(key) = current {
        let key = objects::resolve_address(w, key);
        ensure!(seen.insert(key), "tree parent cycle");
        let o = w.objects.get(key).context("unknown subject context")?;
        subjects.extend(
            objects::stored_reference_targets(w, &o.kind, &o.current().data)?
                .into_iter()
                .filter(|(_, _, subject)| *subject)
                .map(|(_, target, _)| target.split_once(':').unwrap().1.to_owned()),
        );
        current = o.current().parent.as_deref();
    }
    subjects.sort();
    subjects.dedup();
    Ok(subjects)
}

pub(crate) fn check_projects(
    w: &Workspace,
    parent: Option<&str>,
    projects: &[String],
) -> Result<()> {
    let mut parent = parent;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(key) = parent {
        ensure!(seen.insert(key), "tree parent cycle");
        let o = w.objects.get(key).context("unknown Task ancestor")?;
        if let Some((_, r)) = objects::verification_context(w, key)
            && let Some(scope) = &r.verification_scope
        {
            ensure!(
                projects.iter().all(|p| scope.subject_keys.contains(p)),
                "Task reference is outside the agreed verification subject"
            );
        }
        parent = o.current().parent.as_deref();
    }
    Ok(())
}
