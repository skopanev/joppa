//! Part of `objects`: the parent cycle check and the field readers of a save. The checks of
//! the relations of a save are in relations_check.rs.
use super::*;

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
            | "verification"
            | "work_ready"
            | "evidence"
    )
}
