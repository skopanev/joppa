//! Part of `tasks`: `validate_claim` and the items after it, moved from tasks.rs unchanged.
use super::*;

// Existing journal claims can predate the limit on active work.
pub(crate) fn validate_claim(w: &Workspace, actor: &str, key: &str, t: &Task) -> Result<()> {
    ensure!(
        !crate::planning::awaiting_publication(w, key, t),
        "Task is awaiting planning publication"
    );
    ensure!(
        w.members.contains_key(actor),
        "claimant is not a workspace member"
    );
    ensure!(t.blocked.is_none(), "{BLOCKED}");
    ensure!(holder(w, t).is_none(), "task is already claimed");
    ensure!(t.done.is_none(), "task is already completed");
    ensure!(t.retired.is_none(), "task is retired");
    parent_ready(w, actor, key, &t.purpose)?;
    ensure!(
        !review_required(w, key, t),
        "task needs owner review after its parent revision changed"
    );
    ensure!(
        !t.contract.trim().is_empty(),
        "Task needs a technical contract before claim"
    );
    if !objects::work_ready(w, key) {
        let target = objects::unready_dependency(w, key)
            .map_or_else(String::new, |target| format!(": {target}"));
        anyhow::bail!("Task has an unmet dependency or a changed dependency revision{target}");
    }
    check_projects(
        w,
        w.objects[key].current().parent.as_deref(),
        &subjects(w, key)?,
    )?;
    if t.purpose == "decision" {
        commands::human(w, actor)?;
        ensure!(
            w.objects.get(key).context("unknown Task")?.current().data["owner"] == actor,
            "designated Task owner must claim a decision task"
        );
    }
    Ok(())
}

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
