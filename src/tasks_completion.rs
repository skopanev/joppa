//! The checks before a held Task is done: its evidence, its commit, its claim, its review,
//! its dependencies and the decision that a decision Task needs.
use super::*;

/// Check that `actor` may complete the Task `key` with `evidence` on `commit`.
#[allow(clippy::too_many_arguments)]
pub(super) fn check(
    w: &Workspace,
    actor: &str,
    key: &str,
    o: &objects::Object,
    t: &Task,
    authority: bool,
    evidence: &str,
    commit: Option<&str>,
) -> Result<()> {
    commands::nonempty(evidence)?;
    // After migrate_test_rules a Task names the git sha where it is ready.
    if w.test_rules_migrated {
        let sha = commit.context(
            "missing field `commit`: complete_task needs the git sha where the Task is ready",
        )?;
        ensure!(
            (7..=64).contains(&sha.len())
                && sha
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "commit must be a lowercase git sha"
        );
    }
    // The claim gave the Task its current revision; a later repin needs a new claim.
    ensure!(
        crate::remote_links::changed_since(w, key, o.current().position, false).is_none(),
        "the remote links changed after the claim; release and claim again"
    );
    ensure!(
        t.assignee.as_deref() == Some(actor)
            || w.replay_claims && !w.persistent_claims && authority,
        "Task is not claimed unfinished work"
    );
    ensure!(
        t.assignee.is_some() && t.done.is_none() && t.retired.is_none(),
        "Task is not claimed unfinished work"
    );
    ensure!(
        !review_required(w, key, t),
        "Task must be claimed, unfinished and reviewed"
    );
    ensure!(
        objects::work_ready(w, key),
        "Task has an unmet dependency or a changed dependency revision"
    );
    check_projects(w, o.current().parent.as_deref(), &subjects(w, key)?)?;
    parent_ready(w, actor, key, &t.purpose)?;
    if t.purpose == "decision"
        && (!w.schema_migrated
            && objects::schema(w).types[&o.kind].parent_gate.as_deref() == Some("ready")
            || objects::schema(w).types[&o.kind].parent_decision)
    {
        let parent = o
            .current()
            .parent
            .as_deref()
            .context("decision task requires a parent")?;
        let (req, r, ac) = objects::criterion_context(w, parent)
            .context("configured parent decision requires a verification evidence provider")?;
        let d = w.requirements[req]
            .comments
            .get(evidence)
            .context("decision task evidence must be a comment id")?;
        ensure!(
            d.kind == "decision"
                && !d.withdrawn
                && d.revision == r.number
                && d.ac.as_deref() == Some(&ac.id),
            "link the decision for this parent and revision"
        );
        ensure!(
            ac.decider().is_none_or(|owner| owner == &d.actor),
            "designated specialist must decide"
        );
    }
    Ok(())
}
