//! Part of `objects`: `validate_links` and the items after it, moved from objects.rs unchanged.
use super::*;

/// `kept` holds the links of the current revision: one of them can keep an absent target.
pub(crate) fn validate_links(
    w: &Workspace,
    key: &str,
    links: &[ObjectLink],
    kept: &[ObjectLink],
) -> Result<()> {
    let mut unique = BTreeSet::new();
    // A dependency names a node: it needs no target revision.
    for link in links {
        ensure!(
            LINK_RELATIONS.contains(&link.relation.as_str()),
            "unknown link relation"
        );
        ensure!(
            unique.insert((&link.target, &link.relation)),
            "duplicate object link"
        );
        let target = w
            .objects
            .get(resolve_address(w, &link.target))
            .context("unknown linked object in this workspace")?;
        ensure!(
            target.current().present || kept.contains(link),
            "linked object is absent from the current tree"
        );
        if let Some(r) = link.revision {
            ensure!(
                r > 0 && r <= target.current().revision,
                "unknown linked revision"
            );
        }
        if link.blocking {
            ensure!(
                link.relation == "depends_on",
                "blocking links must be dependencies"
            );
        }
    }
    fn visit(
        w: &Workspace,
        key: &str,
        changed: &str,
        links: &[ObjectLink],
        stack: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        let key = resolve_address(w, key);
        if done.contains(key) {
            return Ok(());
        }
        ensure!(stack.insert(key.into()), "dependency cycle");
        let current = if key == changed {
            links
        } else {
            &w.objects
                .get(key)
                .context("unknown dependency")?
                .current()
                .links
        };
        for link in current.iter().filter(|l| l.blocking) {
            visit(w, &link.target, changed, links, stack, done)?;
        }
        if let Some(parent) = w
            .objects
            .get(key)
            .and_then(|o| o.current().parent.as_deref())
        {
            visit(w, parent, changed, links, stack, done)?;
        }
        stack.remove(key);
        done.insert(key.into());
        Ok(())
    }
    visit(
        w,
        key,
        key,
        links,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
    )
}

pub(crate) fn parent_cycle(w: &Workspace, key: &str, parent: Option<&str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    seen.insert(key.to_string());
    let mut p = parent;
    while let Some(id) = p {
        let id = resolve_address(w, id);
        ensure!(seen.insert(id.into()), "tree parent cycle");
        p = w
            .objects
            .get(resolve_address(w, id))
            .and_then(|o| o.current().parent.as_deref());
    }
    Ok(())
}

pub(crate) fn string(data: &Value, name: &str) -> Result<String> {
    data[name]
        .as_str()
        .map(str::to_owned)
        .with_context(|| format!("missing text field {name}"))
}

pub(crate) fn optional_string(data: &Value, name: &str) -> Result<Option<String>> {
    match data.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => Ok(Some(
            v.as_str()
                .with_context(|| format!("{name} must be text"))?
                .into(),
        )),
    }
}

pub(crate) fn list(data: &Value, name: &str) -> Result<Vec<String>> {
    serde_json::from_value(data.get(name).cloned().unwrap_or_else(|| json!([])))
        .with_context(|| format!("{name} must contain identifiers"))
}

pub(crate) fn local(parent: Option<&str>, kind: &str) -> Result<String> {
    parent
        .and_then(|v| v.strip_prefix(&format!("{kind}:")))
        .map(str::to_owned)
        .with_context(|| format!("parent must be {kind}:id"))
}

pub(crate) fn fresh_acs(r: &Revision) -> Vec<Ac> {
    r.acs
        .iter()
        .cloned()
        .map(|mut a| {
            a.assessment = None;
            a
        })
        .collect()
}

/// The adapter writes a whole requirement revision, which starts without assessments.
/// Each criterion whose record did not change keeps its readiness; `reset_changed`
/// clears the criteria below a changed title, text, link or parent.
pub(crate) fn keep_assessments(w: &mut Workspace, req: &str) {
    let Some([.., previous, current]) = w
        .requirements
        .get_mut(req)
        .map(|q| q.revisions.as_mut_slice())
    else {
        return;
    };
    commands::carry_assessments(&previous.acs, &mut current.acs);
}

pub(crate) fn revise(
    req: &str,
    r: &Revision,
    capability: Option<String>,
    data: &Value,
    reason: &str,
    acs: Vec<Ac>,
) -> Result<Command> {
    Ok(Command::Revise(crate::commands::ReviseOp {
        req: req.into(),
        expected_revision: r.number,
        capability,
        consumers: list(data, "consumers")?,
        title: string(data, "title")?,
        body: string(data, "body")?,
        owner: string(data, "owner")?,
        reason: reason.into(),
        acs,
        coverage_policy: true,
        carry_tasks: true,
    }))
}

pub(crate) fn native_document_fields(kind: &str) -> Option<&'static [&'static str]> {
    match kind {
        "domain" => Some(&["title", "owner", "description", "archived"]),
        "capability" | "consumer" => Some(&["title", "owner", "description"]),
        "requirement" => Some(&["title", "body", "owner", "consumers"]),
        "ac" => Some(&[
            "title",
            "description",
            "owner",
            "decision_required",
            "depends_on",
            "before_launch",
        ]),
        "task" => Some(&["title", "owner", "contract", "projects", "purpose"]),
        _ => None,
    }
}

pub(crate) fn process_field(field: &str) -> bool {
    matches!(
        field,
        "confirmed"
            | "assessment"
            | "acceptance"
            | "accepted"
            | "runs"
            | "acs"
            | "tasks"
            | "assignee"
            | "claim"
            | "planning_claim"
            | "done"
            | "retired"
            | "blocked"
            | "review_required"
            | "verification"
            | "work_ready"
            | "evidence"
    )
}
